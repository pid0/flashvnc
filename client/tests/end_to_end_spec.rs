extern crate libc;
extern crate tempdir;

use std::process::{Command,Child,ExitStatus};
use std::io;
use std::fs::{File,OpenOptions};
use std::path::{Path,PathBuf};
use std::ffi::CString;
use tempdir::TempDir;
use std::time::{Instant,Duration};

use io::{BufRead,Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::OpenOptionsExt;

const VNC_START_PORT : u32 = 5900;

fn system_decode(bytes : Vec<u8>) -> Option<String> {
    match String::from_utf8(bytes) {
        Ok(string) => Some(string),
        Err(_) => None
    }
}

fn as_c_string(path : &Path) -> CString {
    CString::new(path.as_os_str().as_bytes()).unwrap()
}

fn in_project_dir(path : &str) -> PathBuf {
    let mut ret = PathBuf::from("./");
    ret.push(path);
    ret
}
fn abspath(path : &Path) -> PathBuf {
    let mut ret = PathBuf::new();
    ret.push(std::env::current_dir().unwrap());
    ret.push(path);
    ret
}
fn project_binary(name : &str) -> PathBuf {
    abspath(&in_project_dir(&format!("target/debug/{}", name)))
}

struct Server {
    port : u32,
    process : Child,
    test_program_pid : u32, 
    input : File,
    output : io::BufReader<File>
}
impl Server {
    fn start(temp_dir : &TempDir) -> io::Result<Self> {
        let (process_in, process_out) = Self::make_fifos(temp_dir)?;
        let startup_file_path = temp_dir.path().join("xstartup");
        Self::write_startup_file(&startup_file_path, &process_in, 
                                 &process_out)?;

        let process = Command::new("vncserver")
            .args(&["-fg", "-securitytypes", "none", "-xstartup"])
            .arg(startup_file_path)
            .spawn()?;

        let vnc_screen : u32;
        loop {
            let latest_vnc_screen = in_project_dir("latest-vnc-screen.sh");
            let output = Command::new(latest_vnc_screen).output()?;
            if output.stdout.len() > 0 {
                vnc_screen = system_decode(output.stdout.clone())
                    .unwrap().trim().parse().unwrap();
                break;
            }
        }

        let input = OpenOptions::new().write(true).open(process_in)?;
        let mut output = io::BufReader::new(
            OpenOptions::new().read(true).open(process_out)?);
        let mut line = String::new();
        output.read_line(&mut line).unwrap();

        Ok(Server {
            port: VNC_START_PORT + vnc_screen,
            process: process,
            test_program_pid: line.trim().parse().unwrap(),
            input: input,
            output: output,
        })
    }

    fn write_startup_file(startup_file_path : &Path, 
                          process_in : &Path, 
                          process_out : &Path) -> io::Result<()> {
        let test_program_path = project_binary("server_test_program");
        let mut startup_file = OpenOptions::new().create(true).write(true)
            .mode(0o700)
            .open(&startup_file_path)?;
        startup_file.write_all(b"#!/usr/bin/env bash\n")?;
        startup_file.write_all(b"exec ")?;
        startup_file.write_all(test_program_path.as_os_str().as_bytes())?;
        startup_file.write_all(b" <")?;
        startup_file.write_all(process_in.as_os_str().as_bytes())?;
        startup_file.write_all(b" >")?;
        startup_file.write_all(process_out.as_os_str().as_bytes())?;
        Ok(())
    }

    fn make_fifos(temp_dir : &TempDir) -> io::Result<(PathBuf, PathBuf)> {
        let input_fifo_path = temp_dir.path().join("input");
        let output_fifo_path = temp_dir.path().join("output");

        unsafe {
            assert_eq!(libc::mkfifo(
                    as_c_string(&input_fifo_path).as_ptr(), 0o600), 0);
            assert_eq!(libc::mkfifo(
                    as_c_string(&output_fifo_path).as_ptr(), 0o600), 0);
        }

        Ok((input_fifo_path.to_path_buf(), output_fifo_path.to_path_buf()))
    }

    fn port(&self) -> u32 {
        self.port
    }

    fn event_matches(event : &str, expected_type : &str, 
                     expected_params : &[&str]) -> bool {
        let words : Vec<&str> = event.split_whitespace().collect();
        if words[0] != expected_type {
            return false;
        }
        expected_params.iter().all(|param| words.contains(param))
    }

    fn is_heartbeat_line(line : &str) -> bool {
        line.trim().len() == 0
    }

    fn should_have_received_event(&mut self, event_type : &str, 
                                  expected_params : &[&str],
                                  timeout : Option<Duration>) {
        let timeout = match timeout {
            Some(t) => t,
            None => Duration::from_millis(200)
        };

        let mut line = String::new();
        let start_time = Instant::now();
        loop {
            self.output.read_line(&mut line).unwrap();
            if Instant::now().duration_since(start_time) > timeout {
                assert!(false, "timed out");
            }
            if !Self::is_heartbeat_line(&line) {
                break;
            }
        }

        assert!(Self::event_matches(&line, event_type, expected_params),
            line);
    }

    fn change_screen_size(&mut self, width : u32, height : u32) {
        writeln!(self.input, "change-screen-size {}x{}", width, height)
            .unwrap();
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        unsafe {
            libc::kill(self.test_program_pid as i32, libc::SIGTERM);
        }
        self.process.wait().unwrap();
    }
}

struct Client {
    process : Child
}
impl Client {
    fn start(host : &str, port : u32) -> io::Result<Self> {
        let client_path = project_binary("flashvnc");
        let process = Command::new(client_path)
            .arg(format!("{}:{}", host, port))
            .spawn()?;

        assert!(Self::call_atspi_with_id(process.id(), &["wait"]).success(),
            "no client");

        Ok(Client {
            process: process
        })
    }

    fn call_atspi(&self, args : &[&str]) -> ExitStatus {
        Self::call_atspi_with_id(self.process.id(), args)
    }
    fn call_atspi_with_id(pid : u32, args : &[&str]) -> ExitStatus {
        let atspi_script = in_project_dir("atspi.py");
        Command::new(atspi_script)
            .arg(format!("{}", pid))
            .args(args)
            .status().expect("failed to start AT-SPI script")
    }

    fn interact(&self, args : &[&str]) {
        assert!(self.call_atspi(args).success(), "AT-SPI interaction");
    }
    fn query(&self, args : &[&str]) -> bool {
        let code = self.call_atspi(args).code();
        if code == Some(3) {
            true
        } else if code == Some(4) {
            false
        } else {
            panic!("AT-SPI failed");
        }
    }

    fn press_mouse(&self, button : u32, x : u32, y : u32) {
        self.interact(&["mouse", &format!("b{}p", button),
                      &format!("{}", x), &format!("{}", y)]);
    }

    fn should_have_screen_with_size(&self, width : u32, height : u32) {
        assert!(self.query(&["query-screen-size",
                   &format!("{}", width),
                   &format!("{}", height)]));
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        unsafe {
            libc::kill(self.process.id() as i32, libc::SIGTERM);
        }
        self.process.wait().unwrap();
    }
}

#[test]
fn should_communicate_bidirectionally_with_a_vnc_server() {
    let temp_dir = TempDir::new("flashvnc").unwrap();
    let mut server = Server::start(&temp_dir).unwrap();
    let client = Client::start("localhost", server.port()).unwrap();

    client.press_mouse(1, 54, 50);
    server.should_have_received_event("press", &["button=1", "x=54", "y=50",
                                      "x_root=54", "y_root=50"], None);

    server.change_screen_size(1152, 864);
    std::thread::sleep(Duration::from_millis(50));
    client.should_have_screen_with_size(1152, 864);
}
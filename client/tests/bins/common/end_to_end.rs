// This file is part of flashvnc, a VNC client.
// Copyright 2018 Patrick Plagwitz
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <http://www.gnu.org/licenses/>.

use libc;

use std::process::{Command,Child,ExitStatus,Stdio,ChildStdout};
use std::io;
use std::fs::{File,OpenOptions};
use std::path::{Path,PathBuf};
use std::ffi::CString;
use std::time::{Instant,Duration};

use std::io::{BufRead,Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::OpenOptionsExt;
use std::str::FromStr;

const VNC_START_PORT : u16 = 5900;
pub const TEST_FB_WIDTH : u32 = 800;
pub const TEST_FB_HEIGHT : u32 = 600;

pub const KEY_SHIFT_L : u32 = 0xffe1;
pub const KEY_CTRL_L : u32 = 0xffe3;
pub const KEY_RETURN : u32 = 0xff0d;
pub const KEY_F1 : u32 = 0xffbe;
pub const KEY_F5 : u32 = 0xffc2;
pub const KEY_F6 : u32 = 0xffc3;
pub const KEY_F8 : u32 = 0xffc5;
pub const KEY_SPACE : u32 = ' ' as u32;
pub const KEY_LOWER_B : u32 = 'b' as u32;
pub const KEY_LOWER_C : u32 = 'c' as u32;
pub const KEY_LOWER_P : u32 = 'p' as u32;
pub const KEY_LOWER_U : u32 = 'u' as u32;
pub const KEY_LOWER_Y : u32 = 'y' as u32;
pub const KEY_UPPER_A : u32 = 'A' as u32;

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
    ret.push(::std::env::current_dir().unwrap());
    ret.push(path);
    ret
}
pub enum Build {
    Debug,
    Release
}
fn project_binary(build : Build, name : &str) -> PathBuf {
    let path = match build {
        Build::Debug => format!("target/debug/{}", name),
        Build::Release => format!("target/release/{}", name),
    };
    abspath(&in_project_dir(&path))
}
fn project_dir() -> PathBuf {
    abspath(&in_project_dir(""))
}

#[macro_export]
macro_rules! assert_eq_with_timeout {
    ( $timeout_in_ms:expr, $left_f:expr, $right:expr ) => {
        let start = ::std::time::Instant::now();
        let timeout = ::std::time::Duration::from_millis($timeout_in_ms);
        let mut last = $left_f;
        while last != $right {
            ::std::thread::sleep(::std::time::Duration::from_millis(100));
            if ::std::time::Instant::now().duration_since(start) > timeout {
                panic!("assertion failed (timed out): ‘{:?}’ == ‘{:?}’",
                        last, $right);
            }
            last = $left_f;
        }
    }
}

pub struct VncServer {
    port : u16,
    screen_number : u16,
    process : Child
}
impl VncServer {
    pub fn start_with_xstartup(temp_dir : &Path, xstartup_code : &str) 
        -> io::Result<Self>
    {
        let startup_file_path = temp_dir.join("xstartup");
        Self::write_startup_file(&startup_file_path, xstartup_code)?;

        let process = Command::new("vncserver")
            //TODO use constants here
            .args(&["-fg", "-geometry", "800x600", "-securitytypes", 
                  "none", "-xstartup"])
            .arg(startup_file_path)
            .spawn()?;

        let vnc_screen : u16;
        loop {
            let latest_vnc_screen = in_project_dir("latest-vnc-screen.sh");
            let output = Command::new(latest_vnc_screen).output()?;
            if output.stdout.len() > 0 {
                vnc_screen = system_decode(output.stdout.clone())
                    .unwrap().trim().parse().unwrap();
                break;
            }
        }

        Ok(Self {
            port: VNC_START_PORT + vnc_screen,
            screen_number: vnc_screen,
            process: process
        })
    }

    fn write_startup_file(startup_file_path : &Path, code : &str) 
        -> io::Result<()>
    {
        let mut startup_file = OpenOptions::new().create(true).write(true)
            .mode(0o700)
            .open(&startup_file_path)?;
        startup_file.write_all(b"#!/usr/bin/env bash\n")?;
        startup_file.write_all(code.as_bytes())?;
        Ok(())
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}
impl Drop for VncServer {
    fn drop(&mut self) {
        Command::new("vncserver")
            .args(&["-kill", &format!(":{}", self.screen_number)])
            .status().unwrap();
        self.process.wait().unwrap();
    }
}

pub struct Server {
    vncserver : VncServer,
    test_program_pid : u32, 
    input : File,
    output : io::BufReader<File>
}
impl Server {
    pub fn start(temp_dir : &Path) -> io::Result<Self> {
        let (process_in, process_out) = Self::make_fifos(temp_dir)?;
        let test_program_path = project_binary(Build::Release,
                                               "server_test_program");
        let vncserver = VncServer::start_with_xstartup(
            //TODO bullshit because paths need not be UTF-8
            temp_dir, &format!(r#"
                displayNumber="${{DISPLAY:1}}"
                mouseServerPort=$((5100 + $displayNumber))
                pkill mouseserver
                mouseserver "$mouseServerPort" &
                cd {}
                exec {} <{} >{}
            "#, project_dir().to_str().unwrap(),
                test_program_path.to_str().unwrap(), 
                process_in.to_str().unwrap(), 
                process_out.to_str().unwrap()))?;

        let input = OpenOptions::new().write(true).open(process_in)?;
        let mut output = io::BufReader::new(
            OpenOptions::new().read(true).open(process_out)?);
        let mut line = String::new();
        output.read_line(&mut line).unwrap();

        Ok(Server {
            vncserver: vncserver,
            test_program_pid: line.trim().parse().unwrap(),
            input: input,
            output: output,
        })
    }

    fn make_fifos(temp_dir : &Path) -> io::Result<(PathBuf, PathBuf)> {
        let input_fifo_path = temp_dir.join("input");
        let output_fifo_path = temp_dir.join("output");

        unsafe {
            assert_eq!(libc::mkfifo(
                    as_c_string(&input_fifo_path).as_ptr(), 0o600), 0);
            assert_eq!(libc::mkfifo(
                    as_c_string(&output_fifo_path).as_ptr(), 0o600), 0);
        }

        Ok((input_fifo_path.to_path_buf(), output_fifo_path.to_path_buf()))
    }

    pub fn port(&self) -> u16 {
        self.vncserver.port()
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

    pub fn should_have_received_event(&mut self, event_type : &str, 
                                  expected_params : &[&str],
                                  timeout : Option<Duration>) {
        let timeout = match timeout {
            Some(t) => t,
            None => Duration::from_millis(200)
        };

        let line = match self.get_next_line(timeout) {
            None => panic!("timed out"),
            Some(l) => l
        };

        assert!(Self::event_matches(&line, event_type, expected_params),
            line);
    }

    fn get_next_line(&mut self, timeout : Duration) -> Option<String> {
        let start_time = Instant::now();
        let mut ret = String::new();

        while Self::is_heartbeat_line(&ret) {
            if Instant::now().duration_since(start_time) > timeout {
                return None;
            }
            ret.clear();
            self.output.read_line(&mut ret).unwrap();
        }

        Some(ret)
    }

    pub fn change_screen_size(&mut self, width : u32, height : u32) {
        writeln!(self.input, "change-screen-size {}x{}", width, height)
            .unwrap();
    }

    fn query_two_dimensions(&mut self, command : &str) -> (u32, u32) {
        writeln!(self.input, "{}", command).unwrap();
        let line = match self.get_next_line(Duration::from_secs(2)) {
            None => panic!("no {}", command),
            Some(line) => line
        };
        let words : Vec<&str> = line.split_whitespace().collect();

        (u32::from_str(words[0]).unwrap(), 
         u32::from_str(words[1]).unwrap())
    }

    pub fn should_have_screen_size(&mut self, width : u32, height : u32) {
        let mut screen_size = || {
            self.query_two_dimensions("query-screen-size")
        };

        assert_eq_with_timeout!(2000, screen_size(), (width, height));
    }

    pub fn mouse_position(&mut self) -> (u32, u32) {
        self.query_two_dimensions("query-mouse-position")
    }
    pub fn set_mouse_position(&mut self, x : u32, y : u32) {
        writeln!(self.input, "set-mouse-position {} {}", x, y).unwrap();
    }

    pub fn start_benchmark_animation(&mut self) {
        writeln!(self.input, "show-benchmark").unwrap();
    }

    pub fn show_still_image(&mut self, path : &Path) {
        writeln!(self.input, "show-image {}", path.to_str().unwrap()).unwrap();
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        unsafe {
            libc::kill(self.test_program_pid as i32, libc::SIGTERM);
        }
    }
}

pub struct Client {
    process : Child
}
impl Client {
    pub fn start_with_args(host : &str, port : u16, args : &[&str],
                           build : Build, wait : bool) 
        -> io::Result<Self>
    {
        let client_path = project_binary(build, "flashvnc");
        let process = Command::new(client_path)
            .arg(format!("{}:{}", host, port))
            .args(args)
            .stdout(Stdio::piped())
            .spawn()?;

        if wait {
            assert!(Self::call_atspi_with_id(process.id(), &["wait"]).success(),
                "no client");
        }

        Ok(Client {
            process: process
        })
    }
    pub fn start(host : &str, port : u16) -> io::Result<Self> {
        Self::start_with_args(host, port, &[], Build::Debug, true)
    }

    pub fn stdout(&mut self) -> &mut ChildStdout {
        self.process.stdout.as_mut().unwrap()
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

    fn cause_mouse_button_event(&self, kind : char, button : u32) {
        self.interact(&["mouse", &format!("b{}{}", button, kind)]);
    }
    pub fn press_mouse_at(&self, button : u32, x : i32, y : i32) {
        self.position_mouse(x, y);
        self.cause_mouse_button_event('p', button);
    }
    pub fn press_mouse(&self, button : u32) {
        self.cause_mouse_button_event('p', button);
    }
    pub fn release_mouse_at(&self, button : u32, x : i32, y : i32) {
        self.position_mouse(x, y);
        self.cause_mouse_button_event('r', button);
    }
    pub fn click_mouse_at(&self, button : u32, x : i32, y : i32) {
        self.position_mouse(x, y);
        self.cause_mouse_button_event('c', button);
    }
    pub fn cause_mouse_move_event(&self, kind : &str, x : i32, y : i32) {
        self.interact(&["mouse", "m", kind,
                      &format!("{}", x), &format!("{}", y)]);
    }
    pub fn position_mouse(&self, x : i32, y : i32) {
        self.cause_mouse_move_event("abs", x, y);
    }
    pub fn move_mouse(&self, x : i32, y : i32) {
        self.cause_mouse_move_event("rel", x, y);
    }

    pub fn focus(&self) {
        self.interact(&["focus"]);
    }

    pub fn press_key(&self, key : u32) {
        self.interact(&["key-down", &format!("{}", key)]);
    }
    pub fn release_key(&self, key : u32) {
        self.interact(&["key-up", &format!("{}", key)]);
    }
    pub fn press_and_release_key(&self, key : u32) {
        self.interact(&["key", &format!("{}", key)]);
    }

    pub fn take_screenshot(&self, dest : &Path) {
        self.interact(&["take-screenshot", dest.to_str().unwrap()]);
    }

    pub fn resize_window_content(&self, width : u32, height : u32) {
        self.interact(&["resize", 
                      &format!("{}", width), 
                      &format!("{}", height)]);
    }

    pub fn should_have_screen_with_size(&self, width : u32, height : u32) {
        assert!(self.query(&["query-screen-size",
                   &format!("{}", width),
                   &format!("{}", height)]));
    }

    pub fn turn_on_relative_mouse_mode(&self) {
        self.press_and_release_key(KEY_F8);
        self.press_and_release_key(KEY_F6);
    }
    pub fn turn_on_lossless_compression(&self) {
        self.press_and_release_key(KEY_F8);
        self.press_and_release_key(KEY_F5);
    }
    pub fn turn_on_lossy_compression(&self) {
        self.press_and_release_key(KEY_F8);
        self.press_and_release_key(KEY_F1);
    }

    pub fn wait_for_termination(&mut self) {
        self.process.wait().unwrap();
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


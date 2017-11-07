extern crate libc;
extern crate tempdir;

use tempdir::TempDir;
use std::io;
use io::{BufRead,Read};
use std::str::FromStr;
use std::path::{PathBuf,Path};

#[allow(dead_code)]
mod common;

use common::end_to_end::{VncServer,Server,Client,Build};

const NO_OF_SAMPLES : u32 = 7;

fn print_stats(fps : &[u32]) {
    println!("min: {}", fps.iter().min().unwrap());
    println!("max: {}", fps.iter().max().unwrap());
    println!("mean: {}", fps.iter().sum::<u32>() / NO_OF_SAMPLES);
}

enum Components {
    Client { host : String, port : u16 },
    Server,
    Both
}
impl Components {
//    fn must_start_server(&self) -> bool {
//        match *self {
//            Components::Client => false,
//            _ => true
//        }
//    }
//    fn must_start_client(&self) -> bool {
//        match *self {
//            Components::Server { host: _, port: _ } => false,
//            _ => true
//        }
//    }
}

fn parse_args(args : Vec<String>) -> (Option<PathBuf>, Components)  {
    let mut video_path = None;
    let mut components = Components::Both;
    let mut i = 0;
    loop {
        if i >= args.len() {
            break;
        }

        let arg = &args[i];
        match &arg[..] {
            "-v" => {
                video_path = Some(PathBuf::from(&args[i + 1]));
                i += 2;
                continue;
            },
            "-s" => {
                components = Components::Server;
            },
            "-c" => {
                components = Components::Client {
                    host: args[i + 1].clone(),
                    port: args[i + 2].parse().unwrap() //TODO
                };
                i += 3;
                continue;
            },
            _ => { } //TODO output usage: unknown option
        }
        i += 1;
    }
    (video_path, components)
}

fn start_test_program_server(temp_dir : &Path) -> io::Result<(Box<Drop>, u16)> {
    let mut server = Server::start(temp_dir)?;
    server.start_benchmark_animation();
    let port = server.port();
    Ok((Box::new(server), port))
}

fn start_video_server(temp_dir : &Path, video_path : &Path) 
    -> io::Result<(Box<Drop>, u16)>
{
    let server = VncServer::start_with_xstartup(temp_dir, &format!(r#"
        exec mplayer {}
    "#, video_path.to_str().unwrap()))?;
    let port = server.port();
    Ok((Box::new(server), port))
}

struct DummyServer;
impl Drop for DummyServer {
    fn drop(&mut self) { }
}
fn dummy_server() -> Box<Drop> {
    Box::new(DummyServer { })
}

fn main() {
    let args : Vec<_> = std::env::args().collect();
    let (video_path, components) = parse_args(args);

    let mut fps = Vec::new();
    let mut computation_fps = Vec::new();
    {
        let temp_dir = TempDir::new("flashvnc").unwrap();
        let (_server, host, port) = 
            if let &Components::Client { ref host, port } = &components {
                (dummy_server(), &host[..], port)
            } else {
                let (server, port) = if let Some(ref video_path) = video_path {
                    start_video_server(temp_dir.path(), video_path).unwrap()
                } else {
                    start_test_program_server(temp_dir.path()).unwrap()
                };
                (server, "localhost", port)
            };

        if let Components::Server = components {
            eprintln!("started server on port {}", port);
            let mut input = Vec::new();
            io::stdin().read(&mut input).unwrap();
        } else {
            //TODO test both Debug and Release?
            let mut client = Client::start_with_args(&host, port,
                &["--benchmark"], Build::Release, false).unwrap();

            let mut fps_stream = io::BufReader::new(client.stdout());
            let mut line = String::new();

            for _ in 0..NO_OF_SAMPLES {
                line.clear();
                fps_stream.read_line(&mut line).unwrap();
                eprint!("{}", line);
                let words : Vec<&str> = line.split_whitespace().collect();
                fps.push(u32::from_str(words[0]).unwrap());
                let parsed : i32 = words[1].parse().unwrap();
                if parsed >= 0 {
                    computation_fps.push(parsed as u32);
                }
            }
        }
    }

    if let Components::Server = components { }
    else {
        println!("");
        println!("samples: {}", NO_OF_SAMPLES);
        println!("\nActual FPS:");
        print_stats(&fps[..]);
        println!("\nFPS disregarding server:");
        print_stats(&computation_fps[..]);
    }
}

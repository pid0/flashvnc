extern crate libc;
extern crate tempdir;

use tempdir::TempDir;
use std::io;
use io::BufRead;
use std::str::FromStr;

#[allow(dead_code)]
mod common;

use common::end_to_end::{Server,Client,Build};

const NO_OF_SAMPLES : u32 = 4;

fn print_stats(fps : &[u32]) {
    println!("min: {}", fps.iter().min().unwrap());
    println!("max: {}", fps.iter().max().unwrap());
    println!("mean: {}", fps.iter().sum::<u32>() / NO_OF_SAMPLES);
}

fn main() {
    let mut fps = Vec::new();
    let mut computation_fps = Vec::new();
    {
        let temp_dir = TempDir::new("flashvnc").unwrap();
        let mut server = Server::start(temp_dir.path()).unwrap();
        server.start_benchmark_animation();
        //TODO test both Debug and Release?
        let mut client = Client::start_with_args("localhost", server.port(),
            &["--benchmark"], Build::Release).unwrap();

        let mut fps_stream = io::BufReader::new(client.stdout());
        let mut line = String::new();

        for _ in 0..NO_OF_SAMPLES {
            line.clear();
            fps_stream.read_line(&mut line).unwrap();
            let words : Vec<&str> = line.split_whitespace().collect();
            fps.push(u32::from_str(words[0]).unwrap());
            computation_fps.push(u32::from_str(words[1]).unwrap());
            eprint!("{}", line);
        }
    }

    println!("");
    println!("samples: {}", NO_OF_SAMPLES);
    println!("\nActual FPS:");
    print_stats(&fps[..]);
    println!("\nFPS disregarding server:");
    print_stats(&computation_fps[..]);
}

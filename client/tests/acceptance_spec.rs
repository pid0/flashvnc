#![feature(integer_atomics)]
#![feature(const_fn)]

#[macro_use]
extern crate spectral;

//std::sync::Once
extern crate flashvnc;

use spectral::prelude::*;

use std::net::{TcpListener,TcpStream,Shutdown};
use std::io::Write;
use std::cell::RefCell;
use std::time::Duration;
use std::sync::atomic::{AtomicU32,Ordering};
use std::thread::JoinHandle;
use std::sync::mpsc;

use flashvnc::protocol::rfb;
use flashvnc::protocol::parsing::Packet;

static PORT : AtomicU32 = AtomicU32::new(0);

fn new_port() -> u32 {
    5910 + PORT.fetch_add(1, Ordering::Relaxed)
}
fn server_string(port : u32) -> String {
    format!("localhost:{}", port)
}

fn socketpair(port : u32) -> (TcpStream, TcpStream) {
    let address = server_string(port);
    let address_clone = address.clone();

    let client_thread = std::thread::spawn(move || {
        TcpStream::connect(address_clone).unwrap()
    });
    let server = TcpListener::bind(address).unwrap()
        .accept().unwrap().0;
    let client = client_thread.join().unwrap();

    (server, client)
}

struct View {
    events_in : mpsc::Receiver<flashvnc::GuiEvent>
}
impl flashvnc::View for View {
    fn get_events(&self) -> &mpsc::Receiver<flashvnc::GuiEvent> {
        &self.events_in
    }
    fn handle_event(&self, _event : flashvnc::ProtocolEvent) {
        unimplemented!()
    }
}

struct Client {
    socket: TcpStream,
    thread: JoinHandle<Result<(), String>>,
    _gui_events : mpsc::Sender<flashvnc::GuiEvent>,
    _server_port : u32
}
impl Client {
    fn launch() -> Self {
        let port = new_port();

        let (gui_events, gui_events_receiver) = mpsc::channel();
        let view = View {
            events_in: gui_events_receiver
        };
        let thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            flashvnc::socket_thread_main(&server_string(port), view)
        });

        let server = TcpListener::bind(&server_string(port)).unwrap();
        let (client, _) = server.accept().unwrap();
        client.set_read_timeout(Some(Duration::from_secs(2))).unwrap();

        Client {
            socket: client,
            thread: thread,
            _gui_events: gui_events,
            _server_port: port
        }
    }

    fn should_exit_with_error(self) -> String {
        self.thread.join().expect("should not panic")
            .expect_err("should exit with error")
    }
    fn join(self) {
        self.socket.shutdown(Shutdown::Both).unwrap();
        self.thread.join().unwrap().unwrap_or(())
    }

    fn negotiate_version(&mut self) {
        rfb::ProtocolVersion {
            string: String::from("RFB 003.008\n")
        }.write(&mut self.socket).unwrap();
        //TODO read response and return?
    }
}

#[test]
fn should_respond_with_rfb_version_3_8() {
    let mut client = Client::launch();

    rfb::ProtocolVersion {
        string: String::from("RFB 003.008\n")
    }.write(&mut client.socket).unwrap();

    let buffer = RefCell::new(Vec::new());
    let response = rfb::ProtocolVersion::parse(&buffer, &client.socket)
        .unwrap();
    assert_eq!(response.string, "RFB 003.008\n");

    client.join();
}

#[test]
fn should_stop_communication_upon_getting_invalid_string_encoding() {
    let mut client = Client::launch();
    let invalid_utf8_byte = 0xffu8;
    let r = 0x52;
    let f = 0x46;
    let b = 0x42;
    let version = [r, f, b, invalid_utf8_byte];
    client.socket.write_all(&version).unwrap();
    for _ in 0..(rfb::PROTOCOL_VERSION_LEN - version.len()) {
        client.socket.write_all(&[0x20]).unwrap();
    }

    let error_message = client.should_exit_with_error();
    assert_that!(error_message).contains("EncodingError");
    assert_that!(error_message).contains("ProtocolVersion");
}

#[test]
fn should_not_continue_with_rfb_versions_other_than_3_8() {
    let mut client = Client::launch();

    rfb::ProtocolVersion {
        string: String::from("RFB 003.007\n")
    }.write(&mut client.socket).unwrap();

    client.socket.shutdown(Shutdown::Both).unwrap();

    let error_message = client.should_exit_with_error();
    assert_that!(error_message).contains("ProtocolVersion");
    assert_that!(error_message).contains("RFB version 3.8");
}

#[test]
fn should_output_an_error_message_if_the_connection_breaks() {
    let mut client = Client::launch();

    client.socket.write_all(&[0x52]).unwrap();
    client.socket.shutdown(Shutdown::Both).unwrap();

    let error_message = client.should_exit_with_error();
    assert_that!(error_message.to_lowercase()).contains("connection");
    assert_that!(error_message.to_lowercase()).contains("broken");
    assert!(!error_message.contains("{"));
}

#[test]
fn should_stop_and_output_an_error_if_it_cant_write_to_the_server() {
    let (mut server, client) = socketpair(new_port());
    client.shutdown(Shutdown::Write).unwrap();
    
    rfb::ProtocolVersion {
        string: String::from("RFB 003.008\n")
    }.write(&mut server).unwrap();

    //TODO refactor
    let (_, rx) = mpsc::channel();
    let view = View {
        events_in: rx
    };
    let error_message = flashvnc::handle_connection(client, view).unwrap_err();
    assert_that!(error_message.to_lowercase()).contains("connection");
    assert_that!(error_message.to_lowercase()).contains("can't write");
    assert!(!error_message.contains("{"));
}

#[test]
fn should_refuse_further_communication_if_server_wants_authentication() {
    let mut client = Client::launch();

    client.negotiate_version();
    rfb::SecurityTypes::SecurityTypesArray(rfb::SecurityTypesArray {
        types: vec![rfb::SEC_TYPE_VNC, rfb::SEC_TYPE_TIGHT]
    }).write(&mut client.socket).unwrap();

    client.socket.shutdown(Shutdown::Write).unwrap();

    let error_message = client.should_exit_with_error();
    assert_that!(error_message).contains("authentication");
}

#[test]
fn should_output_error_message_from_server_after_version_negotiation() {
    let message = "Griping about version";
    let mut client = Client::launch();

    client.negotiate_version();
    rfb::SecurityTypes::ErrorReason(rfb::ErrorReason {
        string: String::from(message)
    }).write(&mut client.socket).unwrap();
    
    client.socket.shutdown(Shutdown::Write).unwrap();

    let client_error = client.should_exit_with_error();
    assert_that!(client_error).contains("version negotiation");
    assert_that!(client_error).contains(message);
}

#[test]
fn should_output_a_server_error_after_security_handshake() {
    let message = "Access Denied";
    let mut client = Client::launch();

    //TODO refactor: share code with test above
    client.negotiate_version();
    rfb::SecurityTypes::SecurityTypesArray(rfb::SecurityTypesArray {
        types: vec![rfb::SEC_TYPE_NONE]
    }).write(&mut client.socket).unwrap();
    rfb::SecurityResult::Failed(rfb::ErrorReason {
        string: String::from(message)
    }).write(&mut client.socket).unwrap();

    client.socket.shutdown(Shutdown::Write).unwrap();

    let client_error = client.should_exit_with_error();
    assert_that!(client_error).contains("security handshake");
    assert_that!(client_error).contains(message);
}

//TODO parsers:
//1. dynamic byte swap
//x. tagged_meta_packet
//3. lazy arrays (writing is mainly the problem, use trait objects?)

//TODO only support true-color, document missing support for SetColorMapEntries

//TODO re-use buffer, what goes wrong currently? -> probably nothing

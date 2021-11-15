extern crate sha1;

use std::collections::HashMap;
use std::convert::TryInto;
use std::ops::Deref;
use std::{env, panic};
use std::path::Path;
use std::fs;
use std::thread;

use std::io::prelude::*;
use std::net::{SocketAddr, TcpListener};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};

const IP_PORT: &str = "localhost:7878";
const ANSWER_TIME_MS: u32 = 5 * 1000;

const T_MAIN: &str = "MAIN";
const T_GAME: &str = "GAME";

#[derive(Debug)]
struct Question {
    id: u8,
    ans: u8, // index of answer (utf8 encoded to avoid conversions)
    text: String, // question and answers separated by newlines
    ws_pack: Vec<u8>, // bytes for sending question via ws 
}

fn main() {
    // setup file stuff
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!("Location of quiz stuff por favor");
        std::process::exit(1);
    }
    let path = Path::new(&args[1]);
    if !path.is_file() {
        println!("Path doesn't point to file!");
        std::process::exit(1);
    }
    let file_contents = fs::read_to_string(path).expect("File read failed");

    // extract questions from file
    let mut questions: Vec<Question> = Vec::with_capacity(10);
    for (id, mut text) in file_contents.split("\n\n").map(|s| s.to_string()).enumerate() {
        let id: u8 = id.try_into().expect("More than 128 questions!");
        let ans: u8 = text.bytes().nth(0).expect("empty question; newlines at end of file?");
        text.remove(0);  // TODO: O(n). Store at the end so I can pop?
        let mut ws_pack = String::with_capacity(2 + text.len());
        ws_pack.push((id + b'0') as char);
        ws_pack.push('\n');
        ws_pack.push_str(&text);
        let ws_pack = ws_packet(ws_pack.as_bytes());
        questions.push(Question{id, ans, text, ws_pack});
    }

    // prepare HTML page
    let html_page = fs::read_to_string("page.html")
        .expect("Failed to read HTML; have page.html in same directory!");
    let http_reponse = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
        html_page.len(),
        html_page,
    );
    
    // prepare stream handles handler
    let streams: Arc<Mutex<Vec<TcpStream>>> = Arc::new(Mutex::new(Vec::with_capacity(20)));
    // TODO: remove type here

    // set up game data and thread
    let game_thread = {
        let streams = Arc::clone(&streams);
        // TODO: I don't think arc is needed anymore, move quesitons into game thread.
        thread::spawn(move || {
            let mut scores: HashMap<SocketAddr, u8> = HashMap::with_capacity(20);

            println!("[{}]: PRESS RETURN TO START QUESTIONS", T_GAME);
            let mut buffer = String::new();
            std::io::stdin().read_line(&mut buffer).unwrap();  // wait for user input
            println!("[{}]: QUESTIONS STARTING", T_GAME);

            for (i, q) in questions.iter().enumerate() {
                println!("[{}]: QUESTION {}", T_GAME, i);

                // send Q to everyone
                for stream in &mut *streams.lock().unwrap() {
                    stream.write(&q.ws_pack).expect("Question send failed");
                }

                thread::sleep_ms(ANSWER_TIME_MS);

                for stream in &mut *streams.lock().unwrap() {
                    let ans = {
                        let mut a_id = q.ans + 1;
                        let mut q_id = q.id + 1;
                        while q_id != q.id {  // ignore answers to old questions
                            let mut buff = [0; 8];
                            let q_id_a_id = match stream.read(&mut buff) {
                                Ok(_) => match ws_parse_incoming(&buff) {
                                    Some(res) => res,
                                    None => (q.id, q.ans+1),  // invalid data
                                },
                                Err(_) => (q.id, q.ans+1),  // no data
                            };
                            q_id = q_id_a_id.0;  // TODO: avoid doing this?
                            a_id = q_id_a_id.1;
                        }
                        a_id
                    };
                    if ans == q.ans {
                        println!("Correct {:?}", stream.peer_addr().unwrap());
                        // correct answer to correct question. +1 point! :)
                        let score = scores
                            .entry(stream.peer_addr().unwrap())
                            .or_insert(0);
                        *score += 1;
                    }
                }
            }

            println!("[{}]: QUESTIONS FINISHED; {} players", T_GAME, scores.len());
            println!("[{}]: FINAL RESULTS:", T_GAME);
            scores.iter().enumerate().for_each(|d| println!("{:?}", d));
            for stream in &mut *streams.lock().unwrap() {
                send_bytes_to_stream(stream, b"\x81\x24Game Finished! Closing Connection...");
            }
            thread::sleep_ms(2000);
            for stream in &mut *streams.lock().unwrap() {
                stream.shutdown(std::net::Shutdown::Both).expect("Shutdown failed");
            }
        })
    };

    // start server
    let listener = TcpListener::bind(IP_PORT).unwrap();
    for stream in listener.incoming() {
        let mut stream = stream.unwrap();

        let mut buffer = [0; 1024];
        stream.read(&mut buffer).unwrap();
        let beg_text = String::from_utf8_lossy(&buffer[..]).deref().to_string();
        //println!("-------RECIEVE------\n{}", beg_text);

        if !beg_text.starts_with("GET /") {
            println!("[{}]: Recieved non-GET request!", T_MAIN);
            println!("{}", beg_text);
        } else if beg_text.starts_with("GET / HTTP") {
            // doesn't need thread; should be pretty fast I think.
            // assuming never more than 100 or so users
            println!("[{}]: Root GET request; serving webpage", T_MAIN);
            send_bytes_to_stream(&mut stream, http_reponse.as_bytes());
        } else if beg_text.starts_with("GET /ws") {
            println!("[{}]: WS GET request; replying with handshake", T_MAIN);
            let streams = Arc::clone(&streams);
            ws_handshake_respond(
                stream.try_clone().unwrap(),
                beg_text,
            );
            stream.set_nonblocking(true).expect("Failed to set nb");
            streams.lock().unwrap().push(stream);
        } else {
            println!("[{}]: Failed to parse GET req", T_MAIN);
        }
    }

    game_thread.join().unwrap();
    println!("[{}]: ALL THREADS JOINED, EXIT", T_MAIN);
}

fn ws_handshake_respond(
    mut stream: TcpStream,
    request_text: String,
) {
    let addr = stream.peer_addr().unwrap(); 
    // connection to websocket, start thing
    let key_in = &request_text.split("Sec-WebSocket-Key: ")
        .nth(1).expect("no ws key")[..24];
    let key_out = base64::encode(sha1::Sha1::from(
        format!("{}258EAFA5-E914-47DA-95CA-C5AB0DC85B11", key_in)
    ).digest().bytes());  // I hate myself
    let response = format!(
        "HTTP/1.1 101 Switching Protocols\r\n\
        Upgrade: websocket\r\n\
        Connection: Upgrade\r\n\
        Sec-WebSocket-Accept: {}\r\n\r\n",
        key_out,
    );
    send_bytes_to_stream(&mut stream, response.as_bytes());
    println!("[{}]: Sent HTTP 101", addr);
    //println!("-------SEND------\n{}", response);
}

fn send_bytes_to_stream(stream: &mut TcpStream, buf: &[u8]) {
    stream.write(buf).unwrap();
    stream.flush().unwrap();
}

fn ws_parse_incoming(buf: &[u8; 8]) -> Option<(u8, u8)> {
    if buf[0] != 129 {return None;}  // fin, text
    if buf[1] != 130 {return None;}  // mask, len=2

    let q_id = buf[2] ^ buf[6];
    let a_id = buf[3] ^ buf[7];

    return Some((q_id, a_id));
}

fn ws_packet(payload: &[u8]) -> Vec<u8> {
    // https://developer.mozilla.org/en-US/docs/Web/API/WebSockets_API/Writing_WebSocket_servers
    let len: usize = payload.len();
    let mut res: Vec<u8> = Vec::with_capacity(len + 12);
    
    res.push(0b1_000_0001);  // defaut header (fin; op1)

    // Payload length
    if len < 2<<6 {
        res.push(len as u8);
    } else if len < 2<<15 {
        res.push(126);  // mask flag = 0
        res.extend_from_slice(&(len as u16).to_be_bytes());
    } else if len < 2<<63 {
        println!("MASSIVE PACKET!");  // should never happen
        res.push(127);  // mask flag = 0
        res.extend_from_slice(&(len as u64).to_be_bytes());
    } else {
        panic!("VERY MASSIVE PACKET!");
    }

    res.extend_from_slice(payload);
    res
}

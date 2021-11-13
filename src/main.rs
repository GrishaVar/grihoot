extern crate sha1;

use std::ops::Deref;
use std::{env, panic};
use std::path::Path;
use std::fs;
use std::time::SystemTime;
use std::hash::{Hash, Hasher};
use std::thread;

use std::io::prelude::*;
use std::net::TcpListener;
use std::net::TcpStream;
use std::sync::Arc;

const IP_PORT: &str = "localhost:7878";


#[derive(Debug)]
struct Question {
    id: u8,
    ans: u8,        // index of answer
    text: String,  // question and answers separated by newlines
}
impl Question {
    // https://developer.mozilla.org/en-US/docs/Web/API/WebSockets_API/Writing_WebSocket_servers
    fn ws_packet(&self) -> Vec<u8> {  // TODO: think about caching

        let len: usize = self.text.len() + 2;  // payload length
        let mut res: Vec<u8> = Vec::with_capacity(len + 12);

        // Flags & opcode
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

        // Payload
        res.push(self.id + b'0');
        res.push(b'\n');
        res.extend_from_slice(self.text.as_bytes());
        return res;
    }
}

#[derive(Debug)]
struct PageData<'a> {
    id: u32,
    question: &'a Question,
    // TODO: time
}

#[derive(Debug)]
struct User {
    pid: u32,
    last_seen: SystemTime,
    score: u32,
}
impl Hash for User {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.pid.hash(state);
    }
}
impl PartialEq for User {
    fn eq(&self, other: &Self) -> bool {
        self.pid == other.pid
    }
}
impl Eq for User {}

struct Query {
    ans: u32,
    pid: u32,
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
    for (i, t) in file_contents.split("\n\n").map(|s| s.to_string()).enumerate() {
        let ans = t.bytes().nth(0).expect("empty question; newlines at end of file?") - b'0';
        let mut tt = t.clone();
        tt.remove(0);  // TODO: remove first char in a better way
        questions.push(Question{id: i as u8, ans, text: tt});
    }
    let questions_arc = Arc::new(questions);
    let page = fs::read_to_string("page.html").unwrap();
    //let mut users: HashMap<u32, User> = HashMap::new();

    // start server
    let listener = TcpListener::bind(IP_PORT).unwrap();
    for stream in listener.incoming() {
        let mut stream = stream.unwrap();
        let questions_arc = Arc::clone(&questions_arc);

        let mut buffer = [0; 1024];
        stream.read(&mut buffer).unwrap();
        let beg_text = String::from_utf8_lossy(&buffer[..]).deref().to_string();
        println!("-------RECIEVE------\n{}", beg_text);

        if     !beg_text.starts_with("GET /")      {println!("NON-GET REQ")}
        else if beg_text.starts_with("GET / HTTP") {send_init_page(&mut stream, &page);}
        else if beg_text.starts_with("GET /ws HTTP") {
            // start WS -> new thread, yada yada
            thread::spawn( || {
                let addr = stream.peer_addr().unwrap();
                println!("---===   NEW THREAD STARTED   ===---\n[serving {:?}]\n", addr);
                handle_connection(
                    stream,
                    questions_arc,
                    beg_text,
                    //&mut users
                );
                println!("---===   THREAD DEAD   ===---\n[serving {:?}]\n", addr);
            });
        } else {println!("STRANGE GET REQ")}

    }
}

fn handle_connection(
    mut stream: TcpStream,
    questions_arc: Arc<Vec<Question>>,
    request_text: String,
    //users: &mut HashMap<u32, User>,
) {
    
    // connection to websocket, start thing
    let key_in = &request_text.split("Sec-WebSocket-Key: ")
        .nth(1).expect("no ws key")[..24];
    let response = format!(
        "HTTP/1.1 101 Switching Protocols\r\n\
        Upgrade: websocket\r\n\
        Connection: Upgrade\r\n\
        Sec-WebSocket-Accept: {}\r\n\r\n",
        ws_response_hash(key_in),
    );
    send_bytes_to_stream(&mut stream, response.as_bytes());
    println!("-------SEND------\n{}", response);

    for q in questions_arc.iter() {
        let send_data = &q.ws_packet();
        send_bytes_to_stream(&mut stream, send_data);
        println!("-------SEND------\n{:?}\n", send_data);
        
        thread::sleep_ms(100);
        let mut buf = [0; 8];
        stream.read(&mut buf).unwrap();
        println!("Revieced WS: {:?}", buf);
        
        if buf[0] != 129 {continue;}  // fin, text
        if buf[1] != 130 {continue;}  // mask, len=2

        let qID = (buf[2] ^ buf[6]) - b'0';
        let aID = (buf[3] ^ buf[7]) - b'0';

        if qID != q.id {println!("WRONG QUESTION ANSWERED");continue;}  // TODO

        println!("Guessed {} for question {}.", aID, qID);
        println!("Correct answer was {}. You guess was {}.\n", q.ans, q.ans==aID);
    }
    println!("all questions finished!");
    loop {};

    /*pid = random::<u32>();
    user = User {
        pid,
        last_seen: SystemTime::now(),
        score: 0,
    };
    users.insert(pid, user);*/
}


fn send_init_page(stream: &mut TcpStream, page: &String) {
    // recieved initial GET for main page
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
        page.len(),
        page,
    );
    send_bytes_to_stream(stream, response.as_bytes());
    println!("-------SEND------\n{}[HTML PAGE]\n", &response[..40]);
}


fn ws_response_hash(recieved: &str) -> String {
    base64::encode(sha1::Sha1::from(
        format!("{}258EAFA5-E914-47DA-95CA-C5AB0DC85B11", recieved)
    ).digest().bytes())  // I hate myself
}


fn send_bytes_to_stream(stream: &mut TcpStream, buf: &[u8]) {
    stream.write(buf).unwrap();
    stream.flush().unwrap();
}



use core::time;
use std::{io, net::UdpSocket};
use base64::{prelude::BASE64_STANDARD, Engine};
use crossterm::{event::{self, poll, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers}};
use ratatui::{buffer::Buffer, layout::Rect, style::Stylize, symbols::border, text::Line, widgets::{Block, Paragraph, Widget}, Frame};
use aes_gcm::{
    aead::{Aead, AeadCore, OsRng},Aes256Gcm, Nonce // Or `Aes128Gcm`
};

use utils::generate_aesgcm;
mod utils;

//building a chat app here
fn main() -> io::Result<()> {
    let mut username = String::new();
    let mut roomkey = String::new();
    let mut port = "9191".to_string();
    for i in 1..std::env::args().len() {
        match std::env::args().nth(i) {
            Some(arg) => {
                match arg.as_str() {
                    "--username" | "-u" => username = std::env::args().nth(i + 1).unwrap(),
                    "--roomkey" | "-r" => roomkey = std::env::args().nth(i + 1).unwrap(),
                    "--port" | "-p" => port = std::env::args().nth(i + 1).unwrap(),
                    _ => {}
                }
            }
            None => {}
        }
    }
    
    let mut terminal = ratatui::init();

    if username.is_empty() {
        username = utils::generate_rnd_str(10);
    }

    let app_result = if roomkey.is_empty() {
        BASE64_STANDARD.encode_string(utils::generate_roomkey(), &mut roomkey);
        App::create_room(username, roomkey).run(&mut terminal)
    }
    else {
        App::join_room(username, roomkey, port).run(&mut terminal)
    };
    
    ratatui::restore();
    app_result
}

struct App {
    username: String,
    roomkey: String,
    roombytes: Vec<u8>,
    roomusers: Vec<Line<'static>>,
    history: Vec<Line<'static>>,
    socket: UdpSocket,
    cipher: Aes256Gcm,
    input: String,
    showkey: bool,
    showusers: bool,
    exit: bool,
}

impl App {

    fn create_room(username: String, roomkey: String) -> Self {
        Self {
            username: username.clone(),
            roomkey: roomkey.clone(),
            roombytes: roomkey.as_bytes()[..32].to_vec(),
            roomusers: vec![],
            history: Vec::new(),
            socket: UdpSocket::bind("127.0.0.1:9090").unwrap(),
            cipher: generate_aesgcm(roomkey),
            input: String::new(),
            showkey: false,
            showusers: false,
            exit: false,
        }
    }

    fn join_room(username: String, roomkey: String, port: String) -> Self {
        Self {
            username: username.clone(),
            roomkey: roomkey.clone(),
            roombytes: roomkey.as_bytes()[..32].to_vec(),
            roomusers: vec![],
            history: Vec::new(),
            socket: UdpSocket::bind(format!("127.0.0.1:{}", port)).unwrap(),
            cipher: generate_aesgcm(roomkey),
            input: String::new(),
            showkey: false,
            showusers: false,
            exit: false,
        }
    }

    fn run(&mut self, terminal: &mut ratatui::DefaultTerminal) -> io::Result<()> {

        self.socket.connect("127.0.0.1:9595").unwrap();
        self.socket.set_nonblocking(true).unwrap();
        
        let mut buffer = [0; 1024];

        let mut data = self.roombytes.clone();
        data.append(&mut self.username.as_bytes().to_vec());
        self.socket.send(&data).unwrap();

        while !self.exit {            
            match self.socket.recv_from(buffer.as_mut()) {
                Ok((size, _)) => {
                    if size < 12 {
                        let username = String::from_utf8(buffer[..size].as_ref().to_vec()).unwrap();
                        self.roomusers.push(Line::from(username.clone()).red());
                        self.history.append(&mut vec![Line::from(vec![username.to_owned().red(), " joined the room".red()])]);
                    }
                    else{
                        let decrypted = utils::decrypt(&self.cipher, buffer[..size].as_ref()).unwrap();
                        let (username, message) = decrypted.split_once('|').unwrap();
                        self.history.append(&mut vec![Line::from(vec!["[".cyan(), username.to_owned().cyan(), "] ".cyan(), message.to_owned().gray()])]);
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // no incoming data, can do other things
                }
                Err(e) => {
                    println!("Error: {}", e);
                    break;
                }
            }

            terminal.draw(|f| self.draw(f))?;
            self.handle_events().unwrap();
        }
        Ok(())
    }

    fn draw(&self, frame: &mut Frame) {
        frame.render_widget(self, frame.area());
    }

    fn handle_events(&mut self) -> io::Result<()> {
        if poll(time::Duration::from_millis(100))? {
            // It's guaranteed that `read` won't block, because `poll` returned
            // `Ok(true)`.
            match event::read()? {
                Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                    self.handle_key_event(key_event);
                }
                _=> {}
            }
        } else {
            // Timeout expired, no `Event` is available
        }
        Ok(())
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) {
        if key_event.modifiers.contains(KeyModifiers::CONTROL) {
            match key_event.code {
                KeyCode::Char('c') => self.exit(),
                _ => {}
            }
            return
        }
        
        match key_event.code {
            KeyCode::F(1) => self.showusers = !self.showusers,
            KeyCode::F(2) => self.showkey = !self.showkey,
            KeyCode::Enter => {
                let mut encrypted = utils::encrypt(&self.cipher, self.username.clone() + "|" + &self.input);
                let mut data = self.roombytes.clone();
                data.append(&mut encrypted);
                self.socket.send(&data).unwrap();
                self.input.clear();
            },
            KeyCode::Backspace => {
                self.input.pop();
            },
            KeyCode::Char(c) => self.input.push(c),
            _ => {}
        }
    }

    fn exit(&mut self) {
        self.exit = true;
    }

}

impl Widget for &App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::bordered().border_set(border::PLAIN);
        let style = ratatui::style::Style::default().fg(ratatui::style::Color::Cyan);

        let mut widthleft = area.width;
        let mut heightleft = area.height;
        
        if self.showkey {
            //widthleft -= 6;
            heightleft -= 3;
            Paragraph::new(Line::from(self.roomkey.clone()))
                .block(block.to_owned().title(" Room Key "))
                .style(style.to_owned())
                .render(Rect { x: 0, y: 0, width: widthleft, height: 3 }, buf);
        }

        if self.showusers {
            widthleft -= 20;
            let mut users = Vec::new();
            for user in self.roomusers.iter() {
                users.push(Line::from(user.clone().to_string()));
            }
            Paragraph::new(users)
                .block(block.to_owned().title(" Users "))
                .style(style.to_owned())
                .render(Rect { x: 0, y: area.height - heightleft, width: 20, height: heightleft }, buf);
        }

        let mut history = Vec::new();
        for message in &self.history {
            history.push(Line::from(message.to_owned()));
        }
        if history.len() > (heightleft - 6) as usize {
            history.drain(0..(history.len() - (heightleft - 6) as usize));
        }
        Paragraph::new(history)
            .block(block.to_owned().title(Line::from(" ArtiChat ").centered()))
            .style(style.to_owned())
            .render(Rect { x: area.width - widthleft, y: area.height - heightleft, width: widthleft, height: heightleft - 4 }, buf);

        let input = Paragraph::new(self.input.clone());
        input.block(block.title(" Message "))
            .style(style)
            .render(Rect { x: area.width - widthleft, y: area.height - 4, width: widthleft, height: 4 }, buf);
    }
}

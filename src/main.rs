use std::{env, fmt, io};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, Mutex as AsyncMutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use serde::{Serialize, Deserialize};
use std::collections::HashSet;
use std::io::{stdout, Write};
use std::net::SocketAddr;
use std::ops::RangeInclusive;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::sync::broadcast::Receiver;
use tokio::sync::mpsc::Sender;
use tokio::time::{timeout, Instant};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use crossterm::style::{style, Attribute, Color, Stylize};
use crossterm::{QueueableCommand, cursor};
use indexmap::IndexMap;
use rand::distributions::Uniform;
use rand::{Rng, SeedableRng};
use serde::de::DeserializeOwned;
use num2words::Num2Words;

const DIE_FACES: [[&str; 5]; 6] = [
    ["╔═══════╗", "║       ║", "║   *   ║", "║       ║", "╚═══════╝"],
    ["╔═══════╗", "║     * ║", "║       ║", "║ *     ║", "╚═══════╝"],
    ["╔═══════╗", "║     * ║", "║   *   ║", "║ *     ║", "╚═══════╝"],
    ["╔═══════╗", "║ *   * ║", "║       ║", "║ *   * ║", "╚═══════╝"],
    ["╔═══════╗", "║ *   * ║", "║   *   ║", "║ *   * ║", "╚═══════╝"],
    ["╔═══════╗", "║ *   * ║", "║ *   * ║", "║ *   * ║", "╚═══════╝"]
];
const NUMBERS: [[&str; 5]; 10] = [
    ["         ", "         ", "         ", "         ", "         "],
    ["         ", "         ", "         ", "         ", "         "],
    ["         ", "         ", "         ", "         ", "         "],
    ["         ", "         ", "         ", "         ", "         "],
    ["         ", "         ", "         ", "         ", "         "],
    ["         ", "         ", "         ", "         ", "         "],
    ["  █████  ", " ██      ", " ██████  ", " ██   ██ ", "  █████  "],
    [" ███████ ", "      ██ ", "   ████  ", "    ██   ", "   ██    "],
    ["  █████  ", " ██   ██ ", "  █████  ", " ██   ██ ", "  █████  "],
    ["  █████  ", " ██   ██ ", "  ██████ ", "      ██ ", "  █████  "]
];

const CROSS: [&str; 5] = ["         ", "  ██ ██  ", "    █    ", "  ██ ██  ", "         "];

const NUM_DICE: u8 = 2;

#[derive(Serialize, Deserialize, Debug)]
enum StartMessage {
    Join(String),
    VoteStart(bool),
    Exit,
}
/// Types of messages client will send to server
#[derive(Serialize, Deserialize, Debug, Clone)]
enum ServerMessage {
    /// Information about a wager
    Wager(i8, i8),
    /// Player calling a bluff
    CallLiar,
    /// Player calling exact guess
    CallExact,
    /// Player telling server that it's leaving
    Exit,
}
/// Types of messages server will send to client
#[derive(Serialize, Deserialize, Debug, Clone)]
enum ClientMessage {
    /// Indicates that player's dice have been shuffled and are ready
    DiceReady,
    /// Contains shuffled dice
    ShuffleResult(Vec<u8>),
    /// Contains optional dice info of every player
    SendAll(Vec<PlayerSerialize>),
    /// Information of a player's wager
    Wager(String, u8, u8),
    /// Information of a player calling a bluff
    CallLiar(String, u8),
    /// Information of a player calling exact guess
    CallExact(String, u8),
    /// Information that a player correctly guessed exact die count
    ExactCallCorrect(String),
    /// Information that a player has lost all of his dice and can no longer play
    PlayerBustedOut(String),
    /// Information that a player has lost a die
    PlayerLostDie(String),
    /// Information that a player has won
    PlayerWon(String),
    /// Information that it's a new player's turn
    PlayerTurn(String),
    /// Information of a player leaving the game
    PlayerLeft(String),
    /// Information that game started
    StartGame,
    /// Tells player that his name is invalid/taken
    InvalidName,
    /// Tells player that join request is accepted
    AcceptClient(String),
    /// Server timing out client and forced to lose a die
    TimeOutClient(String),
    /// Tells client that another player has voted
    VoteStart(String, bool, u8, u8),
    /// Server kicking client
    Kick(String),
}

// In the future, use fine-grained locking to prevent deadlock
// and to avoid using options
struct PlayerInfo {
    name: String,
    dice: Vec<u8>,
    start_game: bool,
    stream: Option<TcpStream>,
}

#[derive(Debug, Deserialize)]
pub enum LiarsDiceError {
    IoError(String),
    JsonError(String),
    TimeoutError,
    NameTaken,
    Other(String),
}

impl fmt::Display for LiarsDiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LiarsDiceError::IoError(e) => write!(f, "IO Error: {}", e),
            LiarsDiceError::JsonError(e) => write!(f, "JSON Error: {}", e),
            LiarsDiceError::TimeoutError => write!(f, "Timeout occurred"),
            LiarsDiceError::NameTaken => write!(f, "Name is already taken"),
            LiarsDiceError::Other(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for LiarsDiceError {}

impl From<io::Error> for LiarsDiceError {
    fn from(e: io::Error) -> Self {
        LiarsDiceError::IoError(e.to_string())
    }
}

impl From<serde_json::Error> for LiarsDiceError {
    fn from(e: serde_json::Error) -> Self {
        LiarsDiceError::JsonError(e.to_string())
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct PlayerSerialize {
    name: String,
    dice: Option<Vec<u8>>,
}

fn print_die_faces(vec: &Vec<u8>) {
    for i in 0..5 {
        for val in vec.iter() {
            print!("{}{}", style(DIE_FACES[(*val - 1) as usize][i]).on(Color::Yellow).with(Color::Black), style(" ").on(Color::Yellow));
        }
        println!();
    }
}

fn print_quantity(face: usize, quantity: usize) {
    for i in 0..5 {
        print!("{} ", DIE_FACES[face - 1][i]);
        print!("{}", CROSS[i]);
        println!("{}", NUMBERS[quantity][i]);
    }
    println!();
}
const COLOR_ORDER: [Color; 8] = [Color::Blue, Color::Red, Color::Green, Color::Yellow, Color::Cyan, Color::Magenta, Color::Grey, Color::Reset];

fn print_table(vec: &Vec<PlayerSerialize>) {
    // Count up dice
    let mut dice_quantity: Vec<Vec<Color>> = Vec::new();
    for (i, p) in vec.iter().enumerate() {
        dice_quantity.push(Vec::new());
        let dice = p.dice.clone().unwrap();
        for die in dice {
            dice_quantity[(die - 1) as usize].push(COLOR_ORDER[i]);
        }
    }
    for (i, v) in dice_quantity.iter().enumerate() {
        if v.len() > 6 {
            print_quantity(i, v.len());
        } else {
            for col in 0..5 {
                for color in v {
                    print!("{} ", style(DIE_FACES[i][col]).with(*color));
                }
                println!();
            }
        }
        println!()
    }
}

fn print_wager(die_face: u8, die_quantity: u8) {
    let (current_x, current_y) = cursor::position().unwrap();
    let position: (u16, u16) = (current_x, current_y - 1);

    stdout().queue(cursor::MoveTo(position.0, position.1)).unwrap();

    println!("\rDie face: {}; Die quantity: {}", die_face, die_quantity);

    let _ = stdout().flush();
}

fn serialize_message<T: Serialize>(input: &T) -> Vec<u8> {
    let input_string = serde_json::to_string(&input).unwrap();
    let serialized = input_string.as_bytes();
    let length: u32 = serialized.len() as u32;
    let mut vec: Vec<u8> = Vec::with_capacity((length + 4) as usize);
    vec.extend_from_slice(&length.to_le_bytes());
    vec.extend_from_slice(serialized);

    vec
}

async fn deserialize_message<T: DeserializeOwned>(
    stream: &mut TcpStream,
) -> Result<Option<T>, LiarsDiceError> {
    let mut len_buf: [u8; 4] = [0; 4];
    match stream.read_exact(&mut len_buf).await {
        Ok(0) => return Ok(None),
        Ok(_) => {},
        Err(e) => return Err(LiarsDiceError::IoError(e.to_string())),
    }
    let length = u32::from_le_bytes(len_buf) as usize;
    let mut buffer = vec![0; length];
    match stream.read_exact(&mut buffer).await {
        Err(e) => {
            return Err(LiarsDiceError::IoError(e.to_string()))
        },
        Ok(0) => {
            return Err(LiarsDiceError::IoError(format!("Read 0 bytes but expected {length} bytes")));
        }
        Ok(_) => {},
    }
    match serde_json::from_slice::<T>(&buffer[..length]) {
        Ok(message) => Ok(Some(message)),
        Err(_) => Err(LiarsDiceError::JsonError(String::from("Problem deserializing string"))),
    }
}

async fn serialize_players(players_hash: Arc<AsyncMutex<IndexMap<SocketAddr, PlayerInfo>>>, include_dice: bool) -> Vec<PlayerSerialize> {
    let mut players = players_hash.lock().await;
    let mut v = Vec::new();
    for player in players.values_mut() {
        v.push(PlayerSerialize {
            name: player.name.clone(),
            dice: if include_dice { Some(player.dice.clone()) } else { None },
        });
    }

    v
}

enum WordStyle {
    LowerCase,
    UpperCase,
    AllCaps
}

fn num_to_word(num: u8, style: WordStyle) -> String {
    let s = Num2Words::new(num).cardinal().to_words().unwrap();
    match style {
        WordStyle::LowerCase => s,
        WordStyle::UpperCase => {
            let mut chars = s.chars();
            chars.next().unwrap().to_uppercase().collect::<String>() + chars.as_str()
        },
        WordStyle::AllCaps => {
            s.to_uppercase()
        }
    }
}

async fn client_join(
    mut socket: TcpStream,
    addr: SocketAddr,
    players_hash: Arc<AsyncMutex<IndexMap<SocketAddr, PlayerInfo>>>,
    num_players: Arc<AtomicUsize>
) -> Result<(), LiarsDiceError>
{
    let message = deserialize_message(&mut socket).await?.unwrap();

    // Deserialize the received JSON into a Message
    match message {
        StartMessage::Join(name) => {
            // If name already exists, reject client and continue to next request
            if name.is_empty() {
                return Err(LiarsDiceError::Other(String::from("Name cannot be empty")));
            }
            let mut players = players_hash.lock().await;
            if players.values().filter(|p| p.name == name).count() != 0 {
                drop(players);
                eprintln!("Player with name {name} already joined");

                socket.write_all(serialize_message(&ClientMessage::InvalidName).as_slice()).await?;
                return Err(LiarsDiceError::NameTaken);
            }
            drop(players);
            socket.write_all(serialize_message(&ClientMessage::SendAll(serialize_players(players_hash.clone(), false).await)).as_slice()).await?;
            println!("{name} joined");
            // Insert client into hash table
            players = players_hash.lock().await;
            players.insert(addr, PlayerInfo {
                name,
                dice: Vec::new(),
                start_game: false,
                stream: Some(socket),
            });

            num_players.fetch_add(1, Ordering::Relaxed);
            Ok(())
        },
        _ => Ok(())
    }
}

async fn handle_client(
    socket: TcpStream,
    addr: SocketAddr,
    players_hash: Arc<AsyncMutex<IndexMap<SocketAddr, PlayerInfo>>>,
    num_players: Arc<AtomicUsize>,
    num_votes: Arc<AtomicUsize>,
    mut broadcast_rx: Receiver<ClientMessage>,
    mpsc_tx: Sender<ClientMessage>
)
    -> Result<(), LiarsDiceError> {
    // Read data from the socket
    match client_join(socket, addr.clone(), players_hash.clone(), num_players.clone()).await {
        Ok(()) => {
            let mut players = players_hash.lock().await;
            let player = match players.get_mut(&addr) {
                Some(p) => p,
                None => {
                    return Err(LiarsDiceError::Other(format!("{addr} is not in hashmap")));
                }
            };
            if let Err(_) = mpsc_tx.send(ClientMessage::AcceptClient(player.name.clone())).await {
                eprintln!("Failed to send message to server");
            }
        }
        Err(e) => return Err(e),
    }
    let mut retries = 0;
    const MAX_RETRIES: usize = 5;

    #[allow(unused_labels)]
    'super_loop: loop {
        let name = {
            let mut players = players_hash.lock().await;
            let player = players.get_mut(&addr).unwrap();
            player.start_game = false;
            player.dice = Vec::new();
            player.name.clone()
        };
        // Lobby loop
        'lobby_loop: loop {
            if let Ok(msg) = broadcast_rx.try_recv() {
                let serialized: Result<ClientMessage, LiarsDiceError> = match msg {
                    ClientMessage::StartGame => Ok(ClientMessage::StartGame),
                    ClientMessage::PlayerLeft(p_name) => Ok(ClientMessage::PlayerLeft(p_name)),
                    ClientMessage::AcceptClient(p_name) => Ok(ClientMessage::AcceptClient(p_name)),
                    ClientMessage::VoteStart(p_name, p_vote, n_votes, n_players) =>
                        Ok(ClientMessage::VoteStart(p_name, p_vote, n_votes, n_players)),
                    ClientMessage::Kick(p_name) => Ok(ClientMessage::Kick(p_name)),
                    _ => Err(LiarsDiceError::Other(String::from("Unknown message type"))),
                };
                match serialized {
                    Ok(s) => {
                        let mut players = players_hash.lock().await;
                        let mut player = match players.get_mut(&addr) {
                            Some(p) => p,
                            None => {
                                return Err(LiarsDiceError::Other(format!("{addr} is not in hashmap")));
                            },
                        };
                        let mut stream = player.stream.take().unwrap();
                        drop(players);
                        stream.write_all(serialize_message(&s).as_slice()).await?;
                        players = players_hash.lock().await;
                        player = match players.get_mut(&addr) {
                            Some(p) => p,
                            None => {
                                return Err(LiarsDiceError::Other(format!("{addr} is not in hashmap")));
                            },
                        };
                        player.stream = Some(stream);
                        drop(players);

                        match s {
                            ClientMessage::Kick(p_name) if p_name == name => {
                                return Err(LiarsDiceError::Other(String::from("Player kicked")));
                            },
                            ClientMessage::StartGame => {
                                break 'lobby_loop;
                            },
                            _ => {},
                        }
                    }
                    Err(_) => eprintln!("Failed to send message to client"),
                }
            }
            // Take stream and clone name without holding onto the lock
            let timeout_result = {
                let mut players = players_hash.lock().await;
                let mut player = players.get_mut(&addr).unwrap();
                let mut stream = player.stream.take().unwrap();
                drop(players);
                let ret_val = timeout(Duration::from_millis(20), deserialize_message(&mut stream)).await;
                players = players_hash.lock().await;
                player = players.get_mut(&addr).unwrap();
                player.stream = Some(stream);
                ret_val
            };
            // Test connection
            let message_received: Result<Option<StartMessage>, LiarsDiceError> = timeout_result.unwrap_or_else(|_| Ok(None));
            let message_option = match message_received {
                Ok(n) => {
                    if retries > 0 {
                        retries = 0;
                        println!("{}'s connection re-established!", name);
                    }
                    n
                },
                Err(_) => {
                    retries += 1;
                    println!("{}'s connection lost. Retrying... ({}/{})", name, retries, MAX_RETRIES);
                    if retries >= MAX_RETRIES {
                        println!("Max retries reached. Disconnecting client.");
                        println!("{} left", name);
                        let mut players = players_hash.lock().await;
                        players.shift_remove(&addr);
                        num_players.fetch_sub(1, Ordering::SeqCst);
                        return Err(LiarsDiceError::TimeoutError);
                    }
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    continue;
                },
            };
            let message = match message_option {
                Some(m) => m,
                None => continue,
            };
            {
                let mut players = players_hash.lock().await;
                let player = match players.get_mut(&addr) {
                    Some(p) => p,
                    None => {
                        return Err(LiarsDiceError::Other(format!("{addr} is not in hashmap")));
                    }
                };
                // Read message content
                match message {
                    StartMessage::VoteStart(vote) => {
                        if vote == player.start_game {
                            continue
                        } else if vote && !player.start_game {
                            num_votes.fetch_add(1, Ordering::SeqCst);
                        } else if !vote && player.start_game {
                            num_votes.fetch_sub(1, Ordering::SeqCst);
                        }
                        player.start_game = vote;
                        let n_votes = num_votes.load(Ordering::SeqCst);
                        let n_people = num_players.load(Ordering::SeqCst);

                        if let Err(_) = mpsc_tx.send(ClientMessage::VoteStart(player.name.clone(), vote, n_votes as u8, n_people as u8)).await {
                            eprintln!("Failed to send message to server");
                        }

                        print!("{} voted ", player.name);
                        if vote {
                            print!("{}", style("YES").with(Color::Green).attribute(Attribute::Bold).attribute(Attribute::Underlined));
                        } else {
                            print!("{}", style("NO").with(Color::Red).attribute(Attribute::Bold).attribute(Attribute::Underlined));
                        }
                        println!(" to start game ({}/{})", n_votes, n_people);
                    }
                    StartMessage::Exit => {
                        if let Err(_) = mpsc_tx.send(ClientMessage::PlayerLeft(player.name.clone())).await {
                            eprintln!("Failed to send message to server");
                        }
                        println!("{} left", player.name);
                        num_players.fetch_sub(1, Ordering::SeqCst);
                        if player.start_game {
                            num_votes.fetch_sub(1, Ordering::SeqCst);
                        }
                        players.shift_remove(&addr);
                        return Ok(())
                    }
                    _ => continue,
                }
            }
        }
        let mut current_wager = (1, 0);
        'game_loop: loop {
            let timeout_result = timeout(Duration::from_secs(1), broadcast_rx.recv()).await;
            let broadcast_result = match timeout_result {
                Ok(x) => x,
                Err(_) => continue 'game_loop,
            };
            let message = match broadcast_result {
                Ok(b) => {
                    match b {
                        ClientMessage::DiceReady => {
                            current_wager = (1, 0);
                            println!("{} Acquire lock 1", name.clone());
                            let mut players = players_hash.lock().await;
                            let player = players.get_mut(&addr).unwrap();
                            let dice = player.dice.clone();
                            drop(players);
                            println!("{} Drop lock 1", name.clone());
                            Some(ClientMessage::ShuffleResult(dice))
                        },
                        ClientMessage::SendAll(s_players) => {
                            Some(ClientMessage::SendAll(s_players))
                        },
                        ClientMessage::Wager(p_name, die_face, die_quantity) => {
                            current_wager = (die_face, die_quantity);
                            Some(ClientMessage::Wager(p_name, die_face, die_quantity))
                        },
                        ClientMessage::CallLiar(p_name, retort) => {
                            Some(ClientMessage::CallLiar(p_name, retort))
                        },
                        ClientMessage::CallExact(p_name, retort) => {
                            Some(ClientMessage::CallExact(p_name, retort))
                        },
                        ClientMessage::ExactCallCorrect(p_name) => {
                            Some(ClientMessage::ExactCallCorrect(p_name))
                        },
                        ClientMessage::PlayerBustedOut(p_name) => {
                            Some(ClientMessage::PlayerBustedOut(p_name))
                        },
                        ClientMessage::PlayerLostDie(p_name) => {
                            Some(ClientMessage::PlayerLostDie(p_name))
                        },
                        ClientMessage::PlayerWon(p_name) => {
                            Some(ClientMessage::PlayerWon(p_name))
                        },
                        ClientMessage::PlayerTurn(p_name) => {
                            Some(ClientMessage::PlayerTurn(p_name))
                        },
                        _ => None
                    }
                },
                Err(_) => continue 'game_loop,
            };
            match message {
                Some(msg) => {
                    let mut players = players_hash.lock().await;
                    let mut player = players.get_mut(&addr).unwrap();
                    let mut stream = player.stream.take().unwrap();
                    drop(players);
                    stream.write_all(serialize_message(&msg).as_slice()).await?;
                    players = players_hash.lock().await;
                    player = players.get_mut(&addr).unwrap();
                    player.stream = Some(stream);
                    drop(players);
                    match msg {
                        ClientMessage::PlayerTurn(p_name) => {
                            if p_name != name.clone() {
                                continue 'game_loop;
                            }
                        },
                        ClientMessage::PlayerWon(_) => {
                            break 'game_loop;
                        }
                        _ => continue 'game_loop,
                    }
                },
                None => continue 'game_loop
            }
            println!("Awaiting response from {}...", name.clone());
            let time_left = Duration::from_secs(60);
            'await_response: loop {
                let start_time = Instant::now();
                let timeout_result = {
                    // println!("{} Acquire lock 3", name.clone());
                    let mut players = players_hash.lock().await;
                    let mut player = players.get_mut(&addr).unwrap();
                    let mut stream = player.stream.take().unwrap();
                    drop(players);
                    let ret_val = timeout(time_left, deserialize_message(&mut stream)).await;
                    players = players_hash.lock().await;
                    player = players.get_mut(&addr).unwrap();
                    player.stream = Some(stream);
                    ret_val
                };
                // println!("{} Drop lock 3", name.clone());
                let elapsed = start_time.elapsed();
                let _ = time_left.checked_sub(elapsed);
                let message_received: Result<Option<ClientMessage>, LiarsDiceError> = match timeout_result {
                    Ok(x) => x,
                    Err(_) => {
                        let mut players = players_hash.lock().await;
                        let mut player = players.get_mut(&addr).unwrap();
                        let mut stream = player.stream.take().unwrap();
                        drop(players);
                        let _ = stream.write_all(serialize_message(&ClientMessage::TimeOutClient(name.clone())).as_slice()).await;
                        let _ = stream.flush().await;
                        players = players_hash.lock().await;
                        player = players.get_mut(&addr).unwrap();
                        player.stream = Some(stream);
                        players.shift_remove(&addr);
                        num_players.fetch_sub(1, Ordering::SeqCst);
                        return Err(LiarsDiceError::TimeoutError);
                    },
                };

                let message: ClientMessage = match message_received {
                    Ok(x) => match x {
                        Some(y) => y,
                        None => continue 'await_response,
                    },
                    Err(_) => continue 'await_response,
                };
                match message {
                    ClientMessage::Wager(_, die_face, die_quantity) => {
                        if !(1..=6).contains(&die_face) || !(die_face > current_wager.0 || die_quantity > current_wager.1) {
                            eprintln!("Invalid wager");
                            continue 'await_response;
                        }
                        println!("{} wagered {} {}'s", name.clone(), num_to_word(die_quantity, WordStyle::LowerCase), die_face);
                        if let Err(_) = mpsc_tx.send(ClientMessage::Wager(name.clone(), die_face, die_quantity)).await {
                            eprintln!("Failed to send message to server");
                        }
                        break 'await_response;
                    },
                    ClientMessage::CallLiar(_, _) => {
                        if let Err(_) = mpsc_tx.send(ClientMessage::CallLiar(name.clone(), 0)).await {
                            eprintln!("Failed to send message to server");
                        }
                        break 'await_response;
                    },
                    ClientMessage::CallExact(_, _) => {
                        if let Err(_) = mpsc_tx.send(ClientMessage::CallExact(name.clone(), 0)).await {
                            eprintln!("Failed to send message to server");
                        }
                        break 'await_response;
                    },
                    _ => {}
                }
            }
        }
    }
}

async fn run_server() -> Result<(), LiarsDiceError> {
    let listener = match TcpListener::bind("0.0.0.0:6969").await {
        Ok(x) => x,
        Err(_e) => {
            return Err(LiarsDiceError::Other(String::from("Failed to bind socket")));
        },
    };

    // These can't be atomicusize, change to asyncmutex. I forgot why these can't be lmao
    let players_hash: Arc<AsyncMutex<IndexMap<SocketAddr, PlayerInfo>>> = Arc::new(AsyncMutex::new(IndexMap::new()));
    let num_players_atomic = Arc::new(AtomicUsize::new(0usize));
    let num_votes_atomic = Arc::new(AtomicUsize::new(0usize));
    // Create a broadcast channel for server-to-clients communication
    let (broadcast_tx, _) = broadcast::channel(32);

    // Create an mpsc channel for clients-to-server communication
    let (server_tx, mut server_rx): (Sender<ClientMessage>, mpsc::Receiver<ClientMessage>) = mpsc::channel(32);

    #[allow(unused_labels)]
    'super_loop: loop {
        num_votes_atomic.store(0, Ordering::SeqCst);
        // Lobby loop
        'lobby_loop: loop {
            if let Ok(msg) = server_rx.try_recv() {
                if let Err(_) = match msg {
                    ClientMessage::VoteStart(p_name, p_vote, n_votes, n_players) =>
                        broadcast_tx.send(ClientMessage::VoteStart(p_name, p_vote, n_votes, n_players)),
                    ClientMessage::AcceptClient(p_name) =>
                        broadcast_tx.send(ClientMessage::AcceptClient(p_name)),
                    ClientMessage::PlayerLeft(p_name) =>
                        broadcast_tx.send(ClientMessage::PlayerLeft(p_name)),
                    ClientMessage::Kick(p_name) =>
                        broadcast_tx.send(ClientMessage::Kick(p_name)),
                    ClientMessage::TimeOutClient(p_name) =>
                        broadcast_tx.send(ClientMessage::TimeOutClient(p_name)),
                    _ => Ok(0),
                } {
                    eprintln!("Failed to send broadcast");
                }
                if num_players_atomic.load(Ordering::SeqCst) > 1 && num_players_atomic.load(Ordering::SeqCst) == num_votes_atomic.load(Ordering::SeqCst) {
                    if let Err(_) = broadcast_tx.send(ClientMessage::StartGame) {
                        eprintln!("Failed to send start broadcast");
                    }
                    break 'lobby_loop;
                }
            }
            let timeout_result = timeout(Duration::from_millis(50), listener.accept()).await;
            let new_client = match timeout_result {
                Ok(x) => x,
                Err(_) => continue,
            };
            if let Ok((socket, addr)) = new_client {
                println!("New connection from: {}", addr);

                let players_hash_clone = players_hash.clone();
                let num_players_clone = num_players_atomic.clone();
                let num_votes_clone = num_votes_atomic.clone();
                let broadcast_rx_clone = broadcast_tx.subscribe();
                let server_tx_clone = server_tx.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_client(
                        socket,
                        addr,
                        players_hash_clone,
                        num_players_clone,
                        num_votes_clone,
                        broadcast_rx_clone,
                        server_tx_clone
                    ).await {
                        eprintln!("Error with client {}: {}", addr, e);
                    }
                });
            }
        }

        { // Acquire players lock
            let mut players = players_hash.lock().await;
            for player in players.values_mut() {
                for _ in 0..NUM_DICE {
                    player.dice.push(0);
                }
            }
        } // Drop lock

        let keys = players_hash.lock().await.keys().cloned().collect::<Vec<_>>();

        let mut remaining_players = num_players_atomic.load(Ordering::SeqCst);
        let mut cur_player_name = String::from("");
        // Game loop
        'game_loop: loop {
            if remaining_players == 1 {
                if let Err(_) = broadcast_tx.send(ClientMessage::PlayerWon(cur_player_name.clone()))
                {
                    eprintln!("Failed to send broadcast");
                }
                break 'game_loop;
            } else if remaining_players == 0 {
                eprintln!("Error, no players remaining. Stopping game and moving to lobby...");
                break 'game_loop;
            }
            let mut cur_player_turn = { // Acquire lock
                let mut rng = rand::rngs::StdRng::from_entropy();
                let dist = Uniform::from(1u8..=6u8);
                let mut players = players_hash.lock().await;
                for player in players.values_mut() {
                    for die in player.dice.iter_mut() {
                        *die = rng.sample(dist);
                    }
                }
                rng.sample(Uniform::from(0..remaining_players))
            }; // Drop lock
            // Set up game
            if let Err(_) = broadcast_tx.send(ClientMessage::DiceReady) {
                eprintln!("Failed to send broadcast");
            }
            #[allow(unused_assignments)]
            let mut prev_player_turn: usize = 0;
            let mut first_turn = true;
            #[allow(unused_assignments)]
            let mut prev_player_name: String = String::from("");
            let mut cur_wager: (u8, u8) = (1, 1); // (face, quantity)
            // Turn loop
            'turn_loop: loop {
                println!("{remaining_players} players remaining");
                prev_player_turn = cur_player_turn;
                prev_player_name = cur_player_name;
                { // Acquire lock
                    let mut players = players_hash.lock().await;
                    let connected_players = num_players_atomic.load(Ordering::SeqCst);
                    loop {
                        cur_player_turn = (cur_player_turn + 1) % connected_players;
                        if !players.get_mut(&keys[cur_player_turn]).unwrap().dice.is_empty() {
                            break;
                        }
                    }
                    cur_player_name = players.get(&keys[cur_player_turn]).unwrap().name.clone();
                } // Drop lock
                println!("Broadcast: It's {}'s turn!", cur_player_name);
                if let Err(_) = broadcast_tx.send(ClientMessage::PlayerTurn(cur_player_name.clone()))
                {
                    eprintln!("Failed to send broadcast");
                }
                #[allow(unused_labels)]
                'await_response: loop {
                    let message_received = server_rx.recv().await;
                    let message = match message_received {
                        Some(x) => {
                            match x.clone() {
                                ClientMessage::TimeOutClient(p_name) if p_name == cur_player_name => {
                                    remaining_players -= 1;
                                    println!("Client timed out");
                                    if let Err(_) = broadcast_tx.send(ClientMessage::TimeOutClient(cur_player_name.clone()))
                                    {
                                        eprintln!("Failed to send broadcast");
                                        break 'turn_loop;
                                    }
                                },
                                _ => {}
                            }
                            x
                        },
                        None => break 'turn_loop,
                    };
                    println!("Got message from task: {:?}", message);
                    match message {
                        ClientMessage::Wager(p_name, die_face, die_quantity) if p_name == cur_player_name => {
                            if let Err(_) = broadcast_tx.send(ClientMessage::Wager(p_name, die_face, die_quantity))
                            {
                                eprintln!("Failed to send broadcast");
                            }
                            cur_wager = (die_face, die_quantity);
                            first_turn = false;
                            tokio::time::sleep(Duration::from_secs(3)).await;
                            break;
                        },
                        ClientMessage::CallLiar(p_name, _) if p_name == cur_player_name && !first_turn => {
                            if let Err(_) = broadcast_tx.send(ClientMessage::CallLiar(p_name.clone(), generate_reveal_val()))
                            {
                                eprintln!("Failed to send broadcast");
                            }
                            if let Err(_) = broadcast_tx.send(ClientMessage::SendAll(serialize_players(players_hash.clone(), true).await))
                            {
                                eprintln!("Failed to send broadcast");
                            }
                            first_turn = false;
                            // Count up total dies
                            let mut all_die: [u8; 6] = [0; 6];
                            { // Acquire lock
                                let mut players = players_hash.lock().await;
                                for player in players.values_mut() {
                                    for die in player.dice.iter_mut() {
                                        all_die[(*die - 1) as usize] += 1;
                                    }
                                }
                            } // Drop lock
                            tokio::time::sleep(Duration::from_secs(3)).await;
                            if cur_wager.1 > all_die[(cur_wager.0 - 1) as usize] {
                                if let Err(_) = broadcast_tx.send(ClientMessage::PlayerLostDie(prev_player_name.clone()))
                                {
                                    eprintln!("Failed to send broadcast");
                                }
                                let mut players = players_hash.lock().await;
                                let player = players
                                    .get_mut(&keys[prev_player_turn])
                                    .unwrap();
                                player.dice.pop();
                                if player.dice.is_empty() {
                                    remaining_players -= 1;
                                    tokio::time::sleep(Duration::from_secs(3)).await;
                                    if let Err(_) = broadcast_tx.send(ClientMessage::PlayerBustedOut(prev_player_name.clone()))
                                    {
                                        eprintln!("Failed to send broadcast");
                                    }
                                }
                                tokio::time::sleep(Duration::from_secs(3)).await;
                            } else {
                                if let Err(_) = broadcast_tx.send(ClientMessage::PlayerLostDie(p_name.clone()))
                                {
                                    eprintln!("Failed to send broadcast");
                                }
                                let mut players = players_hash.lock().await;
                                let player = players
                                    .get_mut(&keys[cur_player_turn])
                                    .unwrap();
                                player.dice.pop();
                                if player.dice.is_empty() {
                                    remaining_players -= 1;
                                    tokio::time::sleep(Duration::from_secs(3)).await;
                                    if let Err(_) = broadcast_tx.send(ClientMessage::PlayerBustedOut(p_name.clone()))
                                    {
                                        eprintln!("Failed to send broadcast");
                                    }
                                }
                            }
                            cur_wager = (1, 1);
                            tokio::time::sleep(Duration::from_secs(3)).await;
                            break 'turn_loop;
                        },
                        ClientMessage::CallExact(p_name, _) if p_name == cur_player_name && !first_turn => {
                            if let Err(_) = broadcast_tx.send(ClientMessage::CallExact(p_name.clone(), generate_reveal_val()))
                            {
                                eprintln!("Failed to send broadcast");
                            }
                            if let Err(_) = broadcast_tx.send(ClientMessage::SendAll(serialize_players(players_hash.clone(), true).await))
                            {
                                eprintln!("Failed to send broadcast");
                            }
                            first_turn = false;
                            // Count up total dies
                            let mut all_die: [u8; 6] = [0; 6];
                            { // Acquire lock
                                let mut players = players_hash.lock().await;
                                for player in players.values_mut() {
                                    for die in player.dice.iter_mut() {
                                        all_die[(*die - 1) as usize] += 1;
                                    }
                                }
                            } // Drop lock
                            tokio::time::sleep(Duration::from_secs(3)).await;
                            if cur_wager.1 == all_die[(cur_wager.0 - 1) as usize] {
                                if let Err(_) = broadcast_tx.send(ClientMessage::ExactCallCorrect(p_name.clone()))
                                {
                                    eprintln!("Failed to send broadcast");
                                }
                            } else {
                                if let Err(_) = broadcast_tx.send(ClientMessage::PlayerLostDie(p_name.clone()))
                                {
                                    eprintln!("Failed to send broadcast");
                                }
                                let mut players = players_hash.lock().await;
                                let player = players
                                    .get_mut(&keys[cur_player_turn])
                                    .unwrap();
                                player.dice.pop();
                                if player.dice.is_empty() {
                                    remaining_players -= 1;
                                    tokio::time::sleep(Duration::from_secs(3)).await;
                                    if let Err(_) = broadcast_tx.send(ClientMessage::PlayerBustedOut(p_name.clone()))
                                    {
                                        eprintln!("Failed to send broadcast");
                                    }
                                }
                            }
                            cur_wager = (1, 1);
                            tokio::time::sleep(Duration::from_secs(3)).await;
                            break 'turn_loop;
                        },
                        _ => {}
                    }
                }
            }
        }
    }
}

enum PreviousPlay {
    Wager,
    CallBluff,
    CallExact,
}

async fn run_client(ip: String) -> Result<(), LiarsDiceError> {
    let mut name = String::new();
    loop {
        println!("Enter your name:");

        match io::stdin()
            .read_line(&mut name) {
            Ok(_) => break,
            Err(error) => println!("error: {}\nTry again", error)
        }
    }

    name = name.trim().to_string();
    if name.is_empty() {
        return Err(LiarsDiceError::Other(String::from("Name cannot be empty")));
    }
    let mut stream = TcpStream::connect(ip).await?;

    let mut players: Vec<PlayerSerialize> = vec![];
    stream.write_all(serialize_message(&StartMessage::Join(name.clone())).as_slice()).await?;

    // Read data from the socket
    let timeout_result = timeout(Duration::from_secs(5), deserialize_message(&mut stream)).await;
    let result: Result<ClientMessage, LiarsDiceError> = match timeout_result {
        Ok(Ok(message)) => Ok(message.unwrap()),
        Err(_) | Ok(Err(_)) => {
            // Timeout occurred
            return Err(LiarsDiceError::TimeoutError)
        }
    };

    // Match on the result to process the client message
    match result? {
        ClientMessage::SendAll(p) => {
            println!("Connected as {}!", name);
            // Assign players
            players = p;
        }
        ClientMessage::InvalidName => {
            return Err(LiarsDiceError::NameTaken);
        }
        _ => {
            return Err(LiarsDiceError::Other(String::from("Unknown error occurred")));
        }
    }
    let mut pressed_keys: HashSet<KeyCode> = HashSet::new();
    let mut retries = 0;
    const MAX_RETRIES: usize = 5;

    // Lobby loop
    #[allow(unused_labels)]
    'super_loop: loop {
        println!("Press 'q' to exit");
        println!("Start game? (y/n)");
        let mut vote = false;
        #[allow(unused_labels)]
        'lobby_loop: loop {
            // Poll for an event (non-blocking, with a timeout)
            if event::poll(Duration::from_millis(1))? {
                if let Event::Key(KeyEvent { code, modifiers, kind, .. }) = event::read()? {
                    match kind {
                        // Handle key press
                        KeyEventKind::Press => {
                            if !pressed_keys.contains(&code) {
                                pressed_keys.insert(code);

                                // Key press logic
                                match code {
                                    KeyCode::Char('q') if modifiers.is_empty() => {
                                        stream
                                            .write_all(serialize_message(&StartMessage::Exit).as_slice())
                                            .await?;
                                        stream.flush().await?;
                                        break;
                                    }
                                    KeyCode::Char('y') => {
                                        if !vote {
                                            vote = true;
                                            stream
                                                .write_all(
                                                    serialize_message(&StartMessage::VoteStart(true))
                                                        .as_slice(),
                                                )
                                                .await?;
                                        }
                                    }
                                    KeyCode::Char('n') => {
                                        if vote {
                                            vote = false;
                                            stream
                                                .write_all(
                                                    serialize_message(&StartMessage::VoteStart(false))
                                                        .as_slice(),
                                                )
                                                .await?;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        // Handle key release
                        KeyEventKind::Release => {
                            pressed_keys.remove(&code);
                        }
                        _ => {}
                    }
                }
            }

            let timeout_result: Result<Result<Option<ClientMessage>, LiarsDiceError>, _> =
                timeout(Duration::from_millis(20), deserialize_message::<ClientMessage>(&mut stream)).await;

            // Test connection
            let message: Result<Option<ClientMessage>, LiarsDiceError> = timeout_result.unwrap_or_else(|_| Ok(None));

            let client_option = match message {
                Ok(n) => {
                    if retries > 0 {
                        retries = 0;
                        println!("Connection with server re-established!");
                    }
                    n
                },
                Err(_) => {
                    retries += 1;
                    println!("{}'s connection lost. Retrying... ({}/{})", name, retries, MAX_RETRIES);
                    if retries >= MAX_RETRIES {
                        println!("Max retries reached. Shutting down...");
                        return Err(LiarsDiceError::TimeoutError);
                    }
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    continue;
                },
            };

            let client_message = match client_option {
                Some(x) => x,
                None => continue,
            };

            // Read message content
            match client_message {
                ClientMessage::StartGame => {
                    println!("Starting game");
                    break;
                },
                ClientMessage::PlayerLeft(p_name) => {
                    println!("{} left", p_name);
                    players.retain(|p| p.name != p_name);
                },
                ClientMessage::AcceptClient(p_name) => {
                    println!("{} joined", p_name);
                    players.push(PlayerSerialize {
                        name: p_name,
                        dice: None
                    });
                },
                ClientMessage::VoteStart(p_name, p_vote, n_votes, n_players) => {
                    print!("{} voted ", p_name);
                    if p_vote {
                        print!("{}", style("YES").with(Color::Green).attribute(Attribute::Bold).attribute(Attribute::Underlined));
                    } else {
                        print!("{}", style("NO").with(Color::Red).attribute(Attribute::Bold).attribute(Attribute::Underlined));
                    }
                    println!(" to start game ({}/{})", n_votes, n_players);
                },
                ClientMessage::Kick(_) => {
                    return Err(LiarsDiceError::Other(String::from("Kicked")));
                },
                _ => {}
            }
        }
        let mut first_turn = true;
        let mut dice: Vec<u8>;
        let mut current_wager: (u8, u8) = (1, 1);
        let mut cur_player_name = String::from("");
        let mut prev_player_name = String::from("");
        let mut prev_play = PreviousPlay::Wager;
        'game_loop: loop {
            'await_loop: loop {
                let timeout_result: Result<Result<Option<ClientMessage>, LiarsDiceError>, _> =
                    timeout(Duration::from_millis(250), deserialize_message::<ClientMessage>(&mut stream)).await;

                // Test connection
                let message: Result<Option<ClientMessage>, LiarsDiceError> = timeout_result.unwrap_or_else(|_| Ok(None));

                let client_option = match message {
                    Ok(n) => {
                        if retries > 0 {
                            retries = 0;
                            println!("Connection with server re-established!");
                        }
                        n
                    },
                    Err(_) => {
                        retries += 1;
                        println!("{}'s connection lost. Retrying... ({}/{})", name, retries, MAX_RETRIES);
                        if retries >= MAX_RETRIES {
                            println!("Max retries reached. Shutting down...");
                            return Err(LiarsDiceError::TimeoutError);
                        }
                        tokio::time::sleep(Duration::from_secs(3)).await;
                        continue 'await_loop;
                    },
                };

                let client_message = match client_option {
                    Some(x) => x,
                    None => continue,
                };

                // Read message content
                // todo!("Write the rest of the cases")
                match client_message {
                    ClientMessage::ShuffleResult(shuffled) => {
                        dice = shuffled;
                        if dice.is_empty() {
                            println!("You're out. You can either leave or wait for a new game to start.");
                        } else {
                            print_die_faces(&dice);
                        }
                        current_wager = (1, 1);
                    },
                    ClientMessage::Wager(p_name, die_face, die_quantity) => {
                        println!("{} wagered {} {}{}", p_name, num_to_word(die_quantity, WordStyle::LowerCase), die_face, if die_quantity > 1 {"'s"} else {""});
                        current_wager = (die_face, die_quantity);
                        first_turn = false;
                    },
                    ClientMessage::CallLiar(p_name, retort) => {
                        println!("{} called {} a liar! {}", p_name, prev_player_name.clone(), generate_reveal(retort));
                        prev_play = PreviousPlay::CallBluff;
                    },
                    ClientMessage::CallExact(p_name, retort) => {
                        println!("{} thinks the wager's just right. {}", p_name, generate_reveal(retort));
                        prev_play = PreviousPlay::CallExact;
                    },
                    ClientMessage::SendAll(s_players) => {
                        players = s_players;
                    }
                    ClientMessage::PlayerLostDie(p_name) => {
                        first_turn = true;
                        match prev_play {
                            PreviousPlay::CallBluff => {
                                if p_name == prev_player_name {
                                    println!("{}'s wager was too high. You lose a die.", p_name);
                                } else {
                                    println!("{} called the bluff wrong. You lose a die.", p_name);
                                }
                            },
                            PreviousPlay::CallExact => {
                                println!("{} was off the mark. You lose a die.", p_name);
                            },
                            _ => {}
                        }
                    },
                    ClientMessage::ExactCallCorrect(p_name) => {
                        first_turn = true;
                        println!("Holy cow! {} was exactly right! You keep your die!", p_name);
                    },
                    ClientMessage::PlayerBustedOut(p_name) => {
                        println!("It is not {}'s day today. You busted out.", p_name);
                    },
                    ClientMessage::PlayerWon(p_name) => {
                        println!("That settles it folks! {} won!", p_name);
                        break 'game_loop;
                    }
                    ClientMessage::PlayerTurn(p_name) => {
                        println!("It's {}'s turn", p_name);
                        prev_player_name = cur_player_name;
                        cur_player_name = p_name.clone();
                        if p_name == name {
                            break 'await_loop;
                        }
                    },
                    ClientMessage::PlayerLeft(p_name) => {
                        println!("{} left", p_name);
                        players.retain(|p| p.name != p_name);
                    },
                    ClientMessage::TimeOutClient(p_name) => {
                        println!("Get your ass outta here, {}! You slowpoke!", p_name);
                        if p_name == name {
                            return Err(LiarsDiceError::TimeoutError);
                        }
                    }
                    _ => {}
                }
            }
            let mut die_face: u8 = current_wager.0;
            let mut die_quantity: u8 = current_wager.1;
            println!("Die face: {}; Die quantity: {}", die_face, die_quantity);
            'turn_loop: loop {
                // Player's turn
                // Poll for an event (non-blocking, with a timeout)
                if event::poll(Duration::from_millis(1))? {
                    if let Event::Key(KeyEvent { code, kind, .. }) = event::read()? {
                        match kind {
                            // Handle key press
                            KeyEventKind::Press => {
                                if !pressed_keys.contains(&code) {
                                    pressed_keys.insert(code);

                                    // Key press logic
                                    match code {
                                        KeyCode::Char('w') if die_face < 6 => {
                                            die_face += 1;
                                            print_wager(die_face, die_quantity);
                                        },
                                        KeyCode::Char('s') if die_face > 1 => {
                                            die_face -= 1;
                                            print_wager(die_face, die_quantity);
                                        },
                                        KeyCode::Char('a') if die_quantity > 1 => {
                                            die_quantity -= 1;
                                            print_wager(die_face, die_quantity);
                                        },
                                        KeyCode::Char('d') if die_quantity < u8::MAX => {
                                            die_quantity += 1;
                                            print_wager(die_face, die_quantity);
                                        },
                                        KeyCode::Char('l') if !first_turn => {
                                            let _ = stream
                                                .write_all(
                                                    serialize_message(&ClientMessage::CallLiar(name.clone(), 0))
                                                        .as_slice(),
                                                )
                                                .await;
                                            break 'turn_loop;
                                        },
                                        KeyCode::Char('k') if !first_turn => {
                                            let _ = stream
                                                .write_all(
                                                    serialize_message(&ClientMessage::CallExact(name.clone(), 0))
                                                        .as_slice(),
                                                )
                                                .await;
                                            break 'turn_loop;
                                        }
                                        KeyCode::Enter => {
                                            if first_turn || die_face > current_wager.0 || die_quantity > current_wager.1 {
                                                let _ = stream
                                                    .write_all(
                                                        serialize_message(&ClientMessage::Wager(name.clone(), die_face, die_quantity))
                                                            .as_slice(),
                                                    )
                                                    .await;
                                                break 'turn_loop;
                                            }
                                            println!("Wager is too low!\n");
                                            print_wager(die_face, die_quantity);
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            // Handle key release
                            KeyEventKind::Release => {
                                pressed_keys.remove(&code);
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }
}

const REVEAL_RANGE: RangeInclusive<u8> = 0u8..=5u8;

fn generate_reveal_val() -> u8 {
    let mut rng = rand::rngs::StdRng::from_entropy();
    let dist = Uniform::from(REVEAL_RANGE);
    rng.sample(dist)
}

fn generate_reveal(num: u8) -> String {
    match num {
        0 => String::from("Let's see the count!"),
        1 => String::from("Somebody's bluffing, and there's only one way to find out!"),
        2 => String::from("Everybody raise your cups!"),
        3 => String::from("This game has been going on for too long anyways."),
        4 => String::from("And we've barely started!"),
        5 => String::from("Game's getting too hot for you?"),
        _ => String::from("")
    }
}

#[tokio::main]
async fn main() -> Result<(), LiarsDiceError> {
    let args: Vec<String> = env::args().collect();
    let ip: String = match args.len() {
        2 => args[1].clone().trim().parse().unwrap(),
        _ => return Err(LiarsDiceError::Other(String::from("Error reading arguments")))
    };
    println!("{ip}");

    if ip.is_empty() {
        return Err(LiarsDiceError::Other(String::from("Name cannot be empty")))
    }

    if ip == "host" {
        let task = tokio::spawn(async {
            if let Err(e) = run_server().await {
                eprintln!("Server error: {}", e);
            }
        });
        let _ = task.await;
    } else {
        let ip_clone = ip.clone();
        let task = tokio::spawn(async {
            if let Err(e) = run_client(ip_clone).await {
                eprintln!("Client error: {}", e);
            }
        });
        let _ = task.await;
    }

    println!("Press Enter to exit program");
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .expect("Failed to read line");

    Ok(())
}

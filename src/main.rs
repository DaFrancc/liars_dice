use std::{fmt, io};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, Mutex as AsyncMutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt, Interest};
use tokio::net::{TcpListener, TcpStream};
use serde::{Serialize, Deserialize};
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;
use serde::ser::Error;
use tokio::sync::broadcast::Receiver;
use tokio::sync::mpsc::Sender;
use tokio::time::timeout;
use crossterm::event::{self, Event, KeyCode, KeyEvent};
use serde::de::DeserializeOwned;
use tokio::time::error::Elapsed;

const DIE_FACES: [[&str; 5]; 6] = [
    ["---------", "|       |", "|   *   |", "|       |", "---------"],
    ["---------", "|     * |", "|       |", "| *     |", "---------"],
    ["---------", "|     * |", "|   *   |", "| *     |", "---------"],
    ["---------", "| *   * |", "|       |", "| *   * |", "---------"],
    ["---------", "| *   * |", "|   *   |", "| *   * |", "---------"],
    ["---------", "| *   * |", "| *   * |", "| *   * |", "---------"]
];

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
    /// Contains shuffled die of player
    ShuffleResult(Vec<i8>),
    /// Contains optional die info of every player
    SendAll(Vec<PlayerSerialize>),
    /// Information of a player's wager
    Wager(String, i8, i8),
    /// Information of a player calling a bluff
    CallLiar(String),
    /// Information of a player calling exact guess
    CallExact(String),
    /// Information of a player leaving the game
    PlayerLeft(String),
    /// Information that game started
    StartGame,
    /// Information that it's a new player's turn
    PlayerTurn(String),
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

struct PlayerInfo {
    name: String,
    die: Vec<i8>,
    start_game: bool,
    stream: TcpStream,
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
    die: Option<Vec<i8>>,
}

fn print_die_faces(arr: &[usize; 5]) {
    for i in 0..6 {
        for val in arr {
            print!("{} ", DIE_FACES[val - 1][i]);
        }
        println!();
    }
}

fn serialize_message<T: Serialize>(input: &T) -> Vec<u8> {
    let input_string = serde_json::to_string(&input).unwrap();
    let serialized = input_string.as_bytes();
    let length: u32 = serialized.len() as u32;
    let mut vec: Vec<u8> = Vec::with_capacity(length as usize);
    vec.extend_from_slice(&length.to_le_bytes());
    vec.extend_from_slice(serialized);

    vec
}

async fn deserialize_message<T: DeserializeOwned>(
    stream: &mut TcpStream,
) -> Result<Option<T>, LiarsDiceError> {
    let mut len_buf: [u8; 4] = [0; 4];
    match stream.read_exact(&mut len_buf).await {
        Ok(0) => {return Ok(None)},
        Ok(_) => {},
        Err(e) => {return Err(LiarsDiceError::IoError(e.to_string()))},
    }
    let length = u32::from_le_bytes(len_buf) as usize;
    let mut buffer = vec![0; length];
    stream.read_exact(&mut buffer).await?; // std::io::Error
    let message = serde_json::from_slice::<T>(&buffer[..length])?; // serde_json::Error
    Ok(Some(message))
}

async fn serialize_players(players_hash: Arc<AsyncMutex<HashMap<SocketAddr, PlayerInfo>>>, include_die: bool) -> Vec<PlayerSerialize> {
    let mut players = players_hash.lock().await;
    let mut v = Vec::new();
    for player in players.values_mut() {
        v.push(PlayerSerialize {
            name: player.name.clone(),
            die: if include_die { Some(player.die.clone()) } else { None },
        });
    }

    v
}

async fn client_join(
    mut socket: TcpStream,
    addr: SocketAddr,
    players_hash: Arc<AsyncMutex<HashMap<SocketAddr, PlayerInfo>>>,
    num_players: Arc<AtomicUsize>
) -> Result<(), LiarsDiceError>
{
    let message = deserialize_message(&mut socket).await?.unwrap();

    // Deserialize the received JSON into a Message
    match message {
        StartMessage::Join(name) => {
            // If name already exists, reject client and continue to next request
            let mut players = players_hash.lock().await;
            if players.values().filter(|p| p.name == name).count() != 0 {
                drop(players);
                eprintln!("Player with name {name} already joined");

                socket.write_all(serialize_message(&ClientMessage::InvalidName).as_slice()).await?;
                return Err(LiarsDiceError::NameTaken);
            }
            drop(players);
            socket.write_all(serialize_message(&ClientMessage::SendAll(serialize_players(players_hash.clone(), false).await)).as_slice()).await?;
            socket.flush().await?;
            println!("{name} joined");
            // Insert client into hash table
            players = players_hash.lock().await;
            players.insert(addr, PlayerInfo {
                name,
                die: Vec::new(),
                start_game: false,
                stream: socket,
            });

            num_players.fetch_add(1, Ordering::Relaxed);
            Ok(())
        },
        _ => Ok(())
    }
}

async fn handle_client(
    mut socket: TcpStream,
    addr: SocketAddr,
    players_hash: Arc<AsyncMutex<HashMap<SocketAddr, PlayerInfo>>>,
    num_players: Arc<AtomicUsize>,
    num_votes: Arc<AtomicUsize>,
    mut broadcast_rx: Receiver<ClientMessage>,
    mut mpsc_tx: Sender<ServerMessage>
)
    -> Result<(), LiarsDiceError> {
    // Read data from the socket
    client_join(socket, addr.clone(), players_hash.clone(), num_players.clone()).await?;
    let mut retries = 0;
    const MAX_RETRIES: usize = 5;

    // Lobby loop
    loop {
        // Take stream and clone name without holding onto the lock
        let timeout_result;
        let name;
        {
            let mut players = players_hash.lock().await;
            let player = match players.get_mut(&addr) {
                Some(p) => p,
                None => {
                    return Err(LiarsDiceError::Other(format!("{addr} is not in hashmap")));
                }
            };
            name = player.name.clone();

            timeout_result = timeout(Duration::from_millis(20), deserialize_message(&mut player.stream)).await;
        }
        // Test connection
        let message: Result<StartMessage, LiarsDiceError> = match timeout_result {
            Ok(Ok(n)) => {
                println!("Got some data");
                if retries > 0 {
                    println!("{}'s connection re-established!", name);
                    retries = 0;
                }
                n.unwrap()
            },
            Ok(Err(_)) => {
                retries += 1;
                println!("{}'s connection lost. Retrying... ({}/{})", name, retries, MAX_RETRIES);
                if retries >= MAX_RETRIES {
                    println!("Max retries reached. Disconnecting client.");
                    println!("{} left", name);
                    let mut players = players_hash.lock().await;
                    players.remove(&addr);
                    num_players.fetch_sub(1, Ordering::SeqCst);
                    return Err(LiarsDiceError::TimeoutError);
                }
                tokio::time::sleep(Duration::from_secs(3)).await;
                continue;
            }
            _ => continue,
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
                Ok(StartMessage::VoteStart(vote)) => {
                    if vote && !player.start_game {
                        num_votes.fetch_add(1, Ordering::SeqCst);
                    } else if !vote && player.start_game {
                        num_votes.fetch_sub(1, Ordering::SeqCst);
                    };
                    player.start_game = vote;

                    println!("{} voted {} to start game ({}/{})", player.name, if vote { "YES" } else { "NO" },
                             num_votes.load(Ordering::SeqCst), num_players.load(Ordering::SeqCst));
                }
                Ok(StartMessage::Exit) => {
                    println!("{} left", player.name);
                    num_players.fetch_sub(1, Ordering::SeqCst);
                    if player.start_game {
                        num_votes.fetch_sub(1, Ordering::SeqCst);
                    }
                    players.remove(&addr);
                }
                Err(_) => continue,
                _ => {}
            }
        }

        if let Ok(msg) = broadcast_rx.try_recv() {
            let serialized = match msg {
                ClientMessage::StartGame => serde_json::to_string(&ClientMessage::StartGame),
                ClientMessage::PlayerLeft(p_name) => serde_json::to_string(&ClientMessage::PlayerLeft(p_name)),
                ClientMessage::AcceptClient(p_name) => serde_json::to_string(&ClientMessage::AcceptClient(p_name)),
                ClientMessage::VoteStart(p_name, p_vote, n_players, n_votes) =>
                    serde_json::to_string(&ClientMessage::VoteStart(p_name, p_vote, n_players, n_votes)),
                ClientMessage::Kick(p_name) => serde_json::to_string(&ClientMessage::Kick(p_name)),
                _ => Err(serde_json::Error::custom("Unknown message type"))
            };
            if let Ok(s_msg) = serialized {
                let mut players = players_hash.lock().await;
                let player = match players.get_mut(&addr) {
                    Some(p) => p,
                    None => {
                        return Err(LiarsDiceError::Other(format!("{addr} is not in hashmap")));
                    }
                };
                player.stream.write_all(serialize_message(&s_msg).as_slice()).await?;
            }
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    Ok(())
}

async fn run_server() -> Result<(), LiarsDiceError> {
    let listener = match TcpListener::bind("0.0.0.0:6969").await {
        Ok(x) => x,
        Err(e) => {
            return Err(LiarsDiceError::Other(String::from("Failed to bind socket")));
        },
    };

    // These can't be atomicusize, change to asyncmutex. I forgot why these can't be lmao
    let players_hash: Arc<AsyncMutex<HashMap<SocketAddr, PlayerInfo>>> = Arc::new(AsyncMutex::new(HashMap::new()));
    let num_players = Arc::new(AtomicUsize::new(0usize));
    let num_votes = Arc::new(AtomicUsize::new(0usize));
    // Create a broadcast channel for server-to-clients communication
    let (broadcast_tx, _) = broadcast::channel(16);

    // Create an mpsc channel for clients-to-server communication
    let (server_tx, mut server_rx) = mpsc::channel(16);

    loop {
        let (socket, addr) = listener.accept().await?;
        println!("New connection from: {}", addr);

        let players_hash_clone = players_hash.clone();
        let num_players_clone = num_players.clone();
        let num_votes_clone = num_votes.clone();
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

        let n_players = num_players.load(Ordering::SeqCst);
        let n_votes = num_votes.load(Ordering::SeqCst);

        if n_players > 0 && n_players == n_votes {
            println!("Starting game with {} players", n_players);
            break;
        }
    }

    // loop {
    //
    // }
    Ok(())
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
    let mut stream = TcpStream::connect(ip).await?;

    let mut players: Vec<PlayerSerialize> = vec![];
    stream.write_all(serialize_message(&StartMessage::Join(name.clone())).as_slice()).await?;
    stream.flush().await?;

    // Read data from the socket
    let timeout_result = timeout(Duration::from_secs(5), deserialize_message(&mut stream)).await;
    let result: Result<ClientMessage, LiarsDiceError> = match timeout_result {
        Ok(Ok(message)) => Ok(message.unwrap()), // Success
        Ok(Err(e)) => Err(e),          // Deserialization or reading error
        Err(_) => {
            // Timeout occurred
            return Err(LiarsDiceError::TimeoutError)
        }
    };

    // Match on the result to process the client message
    match result? {
        ClientMessage::SendAll(p) => {
            println!("Connected as {}!", name);
            // Assign players
            let players = p; // Assuming players is in scope or further handled
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

    println!("Press 'q' to exit");
    println!("Start game? (y/n)");
    // Lobby loop
    loop {
        // Poll for an event (non-blocking, with a timeout)
        if event::poll(Duration::from_millis(1))? {
            if let Event::Key(KeyEvent { code, modifiers, .. }) = event::read()? {
                // Check if the key was already pressed
                if !pressed_keys.contains(&code) {
                    continue
                }
                pressed_keys.insert(code);

                // Key press logic
                match code {
                    KeyCode::Char('q') if modifiers.is_empty() => {
                        stream.write_all(serialize_message(&StartMessage::Exit).as_slice()).await?;
                        println!("'q' pressed. Exiting...");
                        break;
                    },
                    KeyCode::Char('y') => {
                        stream.write_all(serialize_message(&StartMessage::VoteStart(true)).as_slice()).await?;
                    },
                    KeyCode::Char('n') => {
                        stream.write_all(serialize_message(&StartMessage::VoteStart(false)).as_slice()).await?;
                    },
                    _ => {
                        println!("Other key pressed: {:?}", code);
                    }
                }
            }
        }

        // Check for released keys
        let released_keys: Vec<KeyCode> = pressed_keys
            .iter()
            .filter(|key| !event::poll(Duration::from_millis(0)).unwrap_or(false))
            .cloned()
            .collect();

        for key in released_keys {
            pressed_keys.remove(&key);
        }

        let timeout_result: Result<Result<Option<ClientMessage>, LiarsDiceError>, _> = timeout(Duration::from_millis(10), deserialize_message(&mut stream)).await;

        // Test connection
        let message = match timeout_result {
            Ok(n) => {
                if retries > 0 {
                    println!("Connection with server re-established!");
                    retries = 0;
                }
                n
            },
            Err(_) => continue,
        };
        let client_message: ClientMessage = match message {
            Ok(x) => x.unwrap(),
            Err(_) => {
                retries += 1;
                println!("{}'s connection lost. Retrying... ({}/{})", name, retries, MAX_RETRIES);
                if retries >= MAX_RETRIES {
                    println!("Max retries reached. Disconnecting client.");
                    return Err(LiarsDiceError::TimeoutError);
                }
                tokio::time::sleep(Duration::from_secs(3)).await;
                continue;
            },
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
                    die: None
                });
            },
            ClientMessage::VoteStart(p_name, p_vote, n_players, n_votes) => {
                println!("{} voted {} to start game ({}/{})", p_name, if p_vote { "YES" } else { "NO" },
                         n_votes, n_players);
            },
            ClientMessage::Kick(_) => {
                return Err(LiarsDiceError::Other(String::from("Kicked")));
            },
            _ => {}
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() {
    let ip: String = String::from("127.0.0.1:6969");
    let mut host_choice = String::new();
    loop {
        println!("Host or join?");

        match io::stdin()
            .read_line(&mut host_choice) {
            Ok(_) => break,
            Err(error) => println!("error: {}\nTry again", error)
        }
    }
    host_choice = host_choice.trim().to_string();

    let server_task = if host_choice == "host" {
        Some(tokio::spawn(async {
            if let Err(e) = run_server().await {
                eprintln!("Server error: {}", e);
            }
        }))
    } else {
        None
    };

    let client_task = if host_choice != "host" {
        Some(tokio::spawn(async {
            if let Err(e) = run_client(ip).await {
                eprintln!("Client error: {}", e);
            }
        }))
    } else {
        None
    };
    // Run both tasks concurrently
    if host_choice == "host" {
        server_task.unwrap().await.unwrap();
    } else {
        // Only run the client task if no server is started
        client_task.unwrap().await.unwrap();
    }

    println!("Press Enter to exit program");
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .expect("Failed to read line");
}

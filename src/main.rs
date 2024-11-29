use std::{env, fmt, io};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, Mutex as AsyncMutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use serde::{Serialize, Deserialize};
use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::sync::broadcast::Receiver;
use tokio::sync::mpsc::Sender;
use tokio::time::{timeout, Instant};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use crossterm::style::{style, Attribute, Color, Stylize};
use indexmap::IndexMap;
use rand::distributions::Uniform;
use rand::distributions::Distribution;
use serde::de::DeserializeOwned;

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
    ShuffleResult(Vec<u8>),
    /// Contains optional die info of every player
    SendAll(Vec<PlayerSerialize>),
    /// Information of a player's wager
    Wager(String, u8, u8),
    /// Information of a player calling a bluff
    CallLiar(String),
    /// Information of a player calling exact guess
    CallExact(String),
    /// Information that a player correctly guessed exact die count
    ExactCallCorrect(String),
    /// Information that a player has lost all of his dice and can no longer play
    PlayerBustedOut(String),
    /// Information that a player has lost a die
    PlayerLostDie(String),
    /// Information that a player has won
    PlayerWon(String),
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
    die: Vec<u8>,
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
    die: Option<Vec<u8>>,
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

async fn serialize_players(players_hash: Arc<AsyncMutex<IndexMap<SocketAddr, PlayerInfo>>>, include_die: bool) -> Vec<PlayerSerialize> {
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
    players_hash: Arc<AsyncMutex<IndexMap<SocketAddr, PlayerInfo>>>,
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

    // Lobby loop
    loop {
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
                Ok(serialized) => {
                    let mut players = players_hash.lock().await;
                    let player = match players.get_mut(&addr) {
                        Some(p) => p,
                        None => {
                            return Err(LiarsDiceError::Other(format!("{addr} is not in hashmap")));
                        }
                    };
                    player.stream.write_all(serialize_message(&serialized).as_slice()).await?;
                    player.stream.flush().await?;
                }
                Err(_) => eprintln!("Failed to send message to client"),
            }
        }
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
                    }
                    else if vote && !player.start_game {
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
    Ok(())
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
    let (broadcast_tx, _) = broadcast::channel(16);

    // Create an mpsc channel for clients-to-server communication
    let (server_tx, mut server_rx): (Sender<ClientMessage>, mpsc::Receiver<ClientMessage>) = mpsc::channel(16);

    'super_loop: loop {
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
                if num_players_atomic.load(Ordering::SeqCst) == num_votes_atomic.load(Ordering::SeqCst) {
                    if let Err(_) = broadcast_tx.send(ClientMessage::StartGame) {
                        eprintln!("Failed to send start broadcast");
                    }
                    break;
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
                for _ in 0..6 {
                    player.die.push(0);
                }
            }
        } // Drop lock

        let keys = players_hash.lock().await.keys().cloned().collect::<Vec<_>>();

        // Game loop
        'game_loop: loop {
            // Set up game
            let mut cur_player_turn = { // Drop lock at end of scope
                let mut players = players_hash.lock().await;
                let mut rng = rand::thread_rng();
                let dist = Uniform::from(1u8..=6u8);
                for player in players.values_mut() {
                    for die in player.die.iter_mut() {
                        *die = dist.sample(&mut rng);
                    }
                }
                Uniform::from(0u8..=players.len() as u8).sample(&mut rng) as usize
            }; // Drop lock
            let mut prev_player_turn: usize = 0;
            let mut remaining_players = num_players_atomic.load(Ordering::SeqCst);
            let mut first_turn = true;
            let mut prev_player_name: String = String::from("");
            let mut cur_player_name: String = String::from("");
            let mut cur_wager: (u8, u8) = (0, 0); // (face, quantity)
            // Turn loop
            'turn_loop: loop {
                if remaining_players == 1 {
                    if let Err(_) = broadcast_tx.send(ClientMessage::PlayerWon(cur_player_name.clone()))
                    {
                        eprintln!("Failed to send broadcast");
                    }
                    break 'game_loop;
                }
                prev_player_turn = cur_player_turn;
                cur_player_turn = (cur_player_turn + 1) % num_players_atomic.load(Ordering::SeqCst);
                prev_player_name = cur_player_name;
                { // Acquire players lock
                    cur_player_name = players_hash
                        .lock()
                        .await
                        .get(&keys[cur_player_turn])
                        .unwrap()
                        .name
                        .clone();
                } // Drop lock
                if let Err(_) = broadcast_tx.send(ClientMessage::PlayerTurn(cur_player_name.clone()))
                {
                    eprintln!("Failed to send broadcast");
                }
                let time_left = Duration::from_secs(30);
                'await_response: loop {
                    let start_time = Instant::now();
                    let timeout_result = timeout(time_left, server_rx.recv()).await;
                    let elapsed = start_time.elapsed();
                    let _ = time_left.checked_sub(elapsed);
                    let message = match timeout_result {
                        Ok(x) => x.unwrap(),
                        Err(_) => {
                            if let Err(_) = broadcast_tx.send(ClientMessage::TimeOutClient(cur_player_name.clone()))
                            {
                                eprintln!("Failed to send broadcast");
                            }
                            let mut players = players_hash.lock().await;
                            let player = players.get_mut(&keys[cur_player_turn]).unwrap();
                            player.die.pop();
                            if player.die.is_empty() {
                                if let Err(_) = broadcast_tx.send(ClientMessage::PlayerBustedOut(cur_player_name.clone()))
                                {
                                    eprintln!("Failed to send broadcast");
                                }
                            }
                            prev_player_name = cur_player_name;
                            break 'turn_loop;
                        },
                    };
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
                        ClientMessage::CallLiar(p_name) if p_name == cur_player_name && !first_turn => {
                            if let Err(_) = broadcast_tx.send(ClientMessage::CallLiar(p_name.clone()))
                            {
                                eprintln!("Failed to send broadcast");
                            }
                            first_turn = false;
                            tokio::time::sleep(Duration::from_secs(3)).await;
                            // Count up total dies
                            let mut all_die: [u8; 6] = [0; 6];
                            { // Acquire lock
                                let mut players = players_hash.lock().await;
                                for player in players.values_mut() {
                                    for die in player.die.iter_mut() {
                                        all_die[(*die-1) as usize] += 1;
                                    }
                                }
                            } // Drop lock
                            if cur_wager.1 < all_die[cur_wager.0 as usize] {
                                if let Err(_) = broadcast_tx.send(ClientMessage::PlayerLostDie(prev_player_name))
                                {
                                    eprintln!("Failed to send broadcast");
                                }
                                let mut players = players_hash.lock().await;
                                let player = players
                                    .get_mut(&keys[prev_player_turn])
                                    .unwrap();
                                player.die.pop();
                                if player.die.is_empty() {
                                    remaining_players -= 1;
                                }
                            } else {
                                if let Err(_) = broadcast_tx.send(ClientMessage::PlayerLostDie(p_name.clone()))
                                {
                                    eprintln!("Failed to send broadcast");
                                }
                                let mut players = players_hash.lock().await;
                                let player = players
                                    .get_mut(&keys[prev_player_turn])
                                    .unwrap();
                                player.die.pop();
                                if player.die.is_empty() {
                                    remaining_players -= 1;
                                }
                            }
                            tokio::time::sleep(Duration::from_secs(3)).await;
                            break;
                        },
                        ClientMessage::CallExact(p_name) if p_name == cur_player_name && !first_turn => {
                            if let Err(_) = broadcast_tx.send(ClientMessage::CallExact(p_name.clone()))
                            {
                                eprintln!("Failed to send broadcast");
                            }
                            first_turn = false;
                            tokio::time::sleep(Duration::from_secs(3)).await;
                            // Count up total dies
                            let mut all_die: [u8; 6] = [0; 6];
                            { // Acquire lock
                                let mut players = players_hash.lock().await;
                                for player in players.values_mut() {
                                    for die in player.die.iter_mut() {
                                        all_die[(*die-1) as usize] += 1;
                                    }
                                }
                            } // Drop lock
                            if cur_wager.1 == all_die[cur_wager.0 as usize] {
                                if let Err(_) = broadcast_tx.send(ClientMessage::ExactCallCorrect(p_name.clone()))
                                {
                                    eprintln!("Failed to send broadcast");
                                }
                            } else {
                                if let Err(_) = broadcast_tx.send(ClientMessage::PlayerLostDie(p_name.clone()))
                                {
                                    eprintln!("Failed to send broadcast");
                                    let mut players = players_hash.lock().await;
                                    let player = players
                                        .get_mut(&keys[cur_player_turn])
                                        .unwrap();
                                    player.die.pop();
                                    if player.die.is_empty() {
                                        remaining_players -= 1;
                                    }
                                }
                            }
                            break;
                        },
                        _ => {}
                    }
                }
            }
        }
    }
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

    println!("Press 'q' to exit");
    println!("Start game? (y/n)");
    let mut vote = false;
    // Lobby loop
    loop {
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
                    die: None
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

    Ok(())
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();
    let ip: String = match args.len() {
        2 => args[1].clone().trim().parse().unwrap(),
        _ => return
    };
    println!("{ip}");

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
}

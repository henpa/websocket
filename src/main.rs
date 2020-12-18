/*

This is basically the same file as https://github.com/seanmonstar/warp/blob/master/examples/websockets_chat.rs

I need to adapt this example, adding the feature of connecting to a local websocket server API running at ws://127.0.0.1:8188/janus
- if the connection fails or drops, we should reconnect (after 1 sec?)
- it should run together with the warp server
- we need to send a keepalive msg every 30 seconds or connection will be dropped

I need an engine to send commands and receive replies to this ws API:
- all commands has an unique transaction string
- all commands are replied with a message with the same transaction string as return

I need to process other random messages received from the ws API
- besides the replies for previous commands (with a transaction string), the ws API can
  also send random events (informing a new event, such a new user logged in, etc)

I can handle the processing of JSON messages and etc, what I cannot do is the websocket client thing and the engine to send/receive messages to it
from the rest of the program.

  ** please search for "HELP 1" and "HELP 2" for further details **

*/


// #![deny(warnings)]
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use futures::{FutureExt, StreamExt};
use tokio::sync::{mpsc, RwLock};
use warp::ws::{Message, WebSocket};
use warp::Filter;

/// Our global unique user id counter.
static NEXT_USER_ID: AtomicUsize = AtomicUsize::new(1);

/// Our state of currently connected users.
///
/// - Key is their id
/// - Value is a sender of `warp::ws::Message`
type Users = Arc<RwLock<HashMap<usize, mpsc::UnboundedSender<Result<Message, warp::Error>>>>>;

#[tokio::main]
async fn main() {
    pretty_env_logger::init();

    // Keep track of all connected users, key is usize, value
    // is a websocket sender.
    let users = Users::default();
    // Turn our "state" into a new Filter...
    let users = warp::any().map(move || users.clone());

    // GET /chat -> websocket upgrade
    let chat = warp::path("chat")
        // The `ws()` filter will prepare Websocket handshake...
        .and(warp::ws())
        .and(users)
        .map(|ws: warp::ws::Ws, users| {
            // This will call our function if the handshake succeeds.
            ws.on_upgrade(move |socket| user_connected(socket, users))
        });

    // GET / -> index html
    let index = warp::path::end().map(|| warp::reply::html(INDEX_HTML));

    let routes = index.or(chat);

    //
    // HELP 1 - instead of only starting the warp server, we also need
    //          to start concurrently our websocket client connection around here
    //
    //          example: 
    //          wsclient_connect();
    //          
    warp::serve(routes).run(([167,99,189,30], 8080)).await;
}

async fn user_connected(ws: WebSocket, users: Users) {
    // Use a counter to assign a new unique ID for this user.
    let my_id = NEXT_USER_ID.fetch_add(1, Ordering::Relaxed);

    eprintln!("new chat user: {}", my_id);

    // Split the socket into a sender and receive of messages.
    let (user_ws_tx, mut user_ws_rx) = ws.split();

    // Use an unbounded channel to handle buffering and flushing of messages
    // to the websocket...
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::task::spawn(rx.forward(user_ws_tx).map(|result| {
        if let Err(e) = result {
            eprintln!("websocket send error: {}", e);
        }
    }));

    // Save the sender in our list of connected users.
    users.write().await.insert(my_id, tx);

    // Return a `Future` that is basically a state machine managing
    // this specific user's connection.

    // Make an extra clone to give to our disconnection handler...
    let users2 = users.clone();

    // Every time the user sends a message, broadcast it to
    // all other users...
    while let Some(result) = user_ws_rx.next().await {
        let msg = match result {
            Ok(msg) => msg,
            Err(e) => {
                eprintln!("websocket error(uid={}): {}", my_id, e);
                break;
            }
        };
        user_message(my_id, msg, &users).await;
    }

    // user_ws_rx stream will keep processing as long as the user stays
    // connected. Once they disconnect, then...
    user_disconnected(my_id, &users2).await;
}

async fn user_message(my_id: usize, msg: Message, users: &Users) {
    // Skip any non-Text messages...
    let msg = if let Ok(s) = msg.to_str() {
        s
    } else {
        return;
    };

    let new_msg = format!("<User#{}>: {}", my_id, msg);

    //
    // HELP 2 - here I need to send commands to the ws API based on users' commands
    //          and process it's result, for example a command to create a room:
    //
    //          // example:
    //          if msg == "createroom/room_id" {
    //              result = wsclient_createroom(room_id);
    //          }
    //
    //          // or a command to kick another user, example:
    //          if msg == "kick/user_id" {
    //              result = wsclient_kick(user_id);
    //          }
    // 


    // New message from this user, send it to everyone else (except same uid)...
    for (&uid, tx) in users.read().await.iter() {
        if my_id != uid {
            if let Err(_disconnected) = tx.send(Ok(Message::text(new_msg.clone()))) {
                // The tx is disconnected, our `user_disconnected` code
                // should be happening in another task, nothing more to
                // do here.
            }
        }
    }
}

async fn user_disconnected(my_id: usize, users: &Users) {
    eprintln!("good bye user: {}", my_id);

    // Stream closed up, so remove from the user list
    users.write().await.remove(&my_id);
}

static INDEX_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
    <head>
        <title>Warp Chat</title>
    </head>
    <body>
        <h1>Warp chat</h1>
        <div id="chat">
            <p><em>Connecting...</em></p>
        </div>
        <input type="text" id="text" />
        <button type="button" id="send">Send</button>
        <script type="text/javascript">
        const chat = document.getElementById('chat');
        const text = document.getElementById('text');
        const uri = 'ws://' + location.host + '/chat';
        const ws = new WebSocket(uri);

        function message(data) {
            const line = document.createElement('p');
            line.innerText = data;
            chat.appendChild(line);
        }

        ws.onopen = function() {
            chat.innerHTML = '<p><em>Connected!</em></p>';
        };

        ws.onmessage = function(msg) {
            message(msg.data);
        };

        ws.onclose = function() {
            chat.getElementsByTagName('em')[0].innerText = 'Disconnected!';
        };

        send.onclick = function() {
            const msg = text.value;
            ws.send(msg);
            text.value = '';

            message('<You>: ' + msg);
        };
        </script>
    </body>
</html>
"#;



fn _wsclient_connect() {

    // 1. we need to connect to the websocket API at ws://127.0.0.1:8188/janus (with header "Sec-WebSocket-Protocol: janus-protocol")
    //    and create some kind of queue to process commands sent by the program
    //
    // 2. after we connect, we need to create a session
    //
    //    session_id = wsclient_createsession();
    //
    // 3. after getting the session_id, we need to create a handle
    //
    //    handle_id = wsclient_createhandle();
    //
    // 4. every 30 seconds, we need to send a keepalive command so our connection to API ws won't be dropped
    //
    //    wsclient_keepalive();
    //
    // 5. sometimes the ws API sends events (json messages) so we need a function to process these events
    //
    //    wsclient_processevent(event);
    //
    // 6. if the connection to the ws API fails or drops, we should repeat steps 1-2-3 again
    //


}

// example createsession
fn _wsclient_createsession() {

    // we need to send this command:
    // {"janus":"create", "apisecret":"api_secret4321", "transaction":"Qs6uJ7jODoJR"}

    // API should reply a json with success such as:
    // {    "janus": "success",    "transaction": "Qs6uJ7jODoJR",    "data": {       "id": 2147901755134278    } }
    // or an error:
    // {    "janus": "error",    "transaction": "Qs6uJ7jODoJR",    "error": {       "code": 403,       "reason": "Unauthorized request (wrong or missing secret/token)"    } }

    // we return the id
    // return id;

}

// example createhandle
fn _wsclient_createhandle() {

    // we need to send this command (with stored session_id):
    // {"janus":"attach", "apisecret":"api_secret4321", "plugin":"janus.plugin.videoroom", "transaction":"tfycla3QP7IR", "session_id": 2147901755134278 }

    // API should reply a json with success such as: (or an error)
    // {    "janus": "success",    "session_id": 2147901755134278,    "transaction": "tfycla3QP7IR",    "data": {       "id": 5256079589400739    } }

    // we return the id
    // return id;
}

// example keepalive
fn _wsclient_keepalive() {
    
    // we need to send this command (with stored session_id)
    // {"janus":"keepalive","apisecret":"api_secret4321", "session_id":2147901755134278, "transaction":"N7vgphoNxsNv"}

    // API should reply a json with ACK
    // {    "janus": "ack",    "session_id": 2147901755134278,    "transaction": "N7vgphoNxsNv" }

}

// example createroom
fn _wsclient_createroom(_room_id: usize) {
    
    // we need to send this command (with stored session_id, handle_id and provided room_id)
    // {"janus":"message", "apisecret":"api_secret4321", "body":{"request":"create", "room": 5555, "admin_key":"admin_key4321"}, "transaction":"zbNqFi0VxiWu", "session_id": 2147901755134278, "handle_id": 5256079589400739 }

    // API should reply a json with success
    // {    "janus": "success",    "session_id": 2147901755134278,    "transaction": "zbNqFi0VxiWu",    "sender": 1574010579734643,    "plugindata": {       "plugin": "janus.plugin.videoroom",       "data": {          "videoroom": "created",          "room": 5555,          "permanent": false       }    } }
    //
    // or an error:
    // {    "janus": "success",    "session_id": 2147901755134278,    "transaction": "zbNqFi0VxiWu",    "sender": 1574010579734643,    "plugindata": {       "plugin": "janus.plugin.videoroom",       "data": {          "videoroom": "event",          "error_code": 429,          "error": "Missing mandatory element (admin_key)"       }    } }

}

// example kick command
fn _wsclient_kick(_user_id: usize) {
    
    // we need to send this command (with stored session_id, handle_id and provided user_id)
    // {"janus":"message","apisecret":"api_secret4321", "body":{"request":"kick", "room": 5555, "secret": "adminpwd", "id": 7295162779679030},"transaction":"MdPPmzvt2HQA","session_id": 2147901755134278 ,"handle_id": 5256079589400739 }

    // API should reply a json with success (or error)
    // {    "janus": "success",    "session_id": 2175572209542756,    "transaction": "MdPPmzvt2HQA",    "sender": 813487213683777,    "plugindata": {       "plugin": "janus.plugin.videoroom",       "data": {          "videoroom": "success"       }    } }
    
}

// example processevent
fn _wsclient_processevent(event: String) {
    
    // we need to process this received event from the ws API

    // {   "janus": "event",   "session_id": 4875230564143493,   "sender": 6705816660265647,   "plugindata": {      "plugin": "janus.plugin.videoroom",      "data": {         "videoroom": "event",         "room": 1234,         "publishers": [            {               "id": 6450855227982898,               "display": "aluno/3",               "audio_codec": "opus",               "video_codec": "vp8",               "talking": false            }         ]      }   }}

}
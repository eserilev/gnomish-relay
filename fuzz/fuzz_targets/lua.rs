//! S8 against the real Lua 5.1 parser: every literal loads back as the same bytes.
//! S9 for each app: a slot body with the input as its text sets only the global of
//! its app, and the text loads back.
#![no_main]

use libfuzzer_sys::fuzz_target;
use mlua::Lua;
use protocol::apps::App;
use protocol::lua::lua_string;
use protocol::slot::{Reply, Status, prepare_replies, slot_body};

const APPS: [(App, &str, &str); 2] = [
    (App::Relay, "GnomishRelay_SlotData", "Timeways_SlotData"),
    (App::Timeways, "Timeways_SlotData", "GnomishRelay_SlotData"),
];

// A fresh state per input: one shared state got 18 times slower as it aged.
fuzz_target!(|data: &[u8]| {
    let lua = Lua::new();
    let version: String = lua.load("return _VERSION").eval().expect("Lua starts");
    assert_eq!(version, "Lua 5.1");

    let mut code = b"return ".to_vec();
    code.extend(lua_string(data));
    let back: mlua::String = lua.load(&code[..]).eval().expect("the literal loads");
    assert_eq!(&*back.as_bytes(), data);

    let reply = Reply {
        chat: data.to_vec(),
        id: 1,
        status: Status::Done,
        text: data.to_vec(),
    };
    let replies = prepare_replies(&[reply]);
    for (app, own, other) in APPS {
        let lua = Lua::new();
        lua.load(&slot_body(app, 0, &replies)[..]).exec().expect("the body loads");
        let body: mlua::Table = lua.globals().get(own).expect("the global of the app");
        let got: mlua::Table = body.get::<mlua::Table>("replies").unwrap().get(1).unwrap();
        let text: mlua::String = got.get("text").unwrap();
        assert_eq!(&*text.as_bytes(), &replies[0].text[..]);
        assert!(lua.globals().get::<mlua::Value>(other).unwrap().is_nil());
    }
});

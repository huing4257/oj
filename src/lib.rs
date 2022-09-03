use actix_web::web::Json;
use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_json;
use serde_json::Value;

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
pub struct Args {
    #[clap(short, long, value_parser)]
    pub config: String,

}

#[derive(Serialize, Deserialize,Clone)]
struct Server {
    #[serde(default = "address_default")]
    bind_address: String,
    #[serde(default = "port_default")]
    bind_port: i32,
}

fn address_default() -> String {
    "127.0.0.1".to_string()
}

fn port_default() -> i32 {
    12345
}

#[derive(Serialize, Deserialize,Clone)]
struct Case {
    score: f64,
    input_file: String,
    answer_file: String,
    time_limit: i64,
    memory_limit: i64,
}

#[derive(Serialize, Deserialize,Clone)]
struct Problem {
    id: i32,
    name: String,
    #[serde(rename = "type")]
    ty: String,
    misc: Option<Value>,
    cases: Vec<Case>,
}

#[derive(Serialize, Deserialize,Clone)]
pub struct Language {
    name: String,
    file_name: String,
    command: Vec<String>,
}

#[derive(Serialize, Deserialize,Clone)]
pub struct Config {
    server: Server,
    problems: Vec<Problem>,
    languages: Vec<Language>,
}


#[derive(Serialize, Deserialize)]
pub struct PostJob {
    source_code: String,
    language: String,
    user_id: String,
    contest_id: String,
    problem_id: String,
}


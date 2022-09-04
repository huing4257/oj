use std::fs;
use std::fs::create_dir;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use actix_web::{get, middleware::Logger, post, web, App, HttpResponse, HttpServer, Responder};
use actix_web::web::{Data, Json};
use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_json;
use serde_json::Value;
use wait_timeout::ChildExt;

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
pub struct Args {
    #[clap(short, long, value_parser)]
    pub config: String,
    #[clap(short, long = "flush-data")]
    pub flush_data: bool,
}

#[derive(Serialize, Deserialize, Clone)]
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

#[derive(Serialize, Deserialize, Clone)]
pub struct Case {
    score: f64,
    input_file: String,
    answer_file: String,
    time_limit: i64,
    memory_limit: i64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Problem {
    pub id: i32,
    name: String,
    #[serde(rename = "type")]
    pub ty: String,
    misc: Option<Value>,
    pub cases: Vec<Case>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Language {
    pub name: String,
    file_name: String,
    pub command: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Config {
    server: Server,
    pub problems: Vec<Problem>,
    pub languages: Vec<Language>,
}


#[derive(Serialize, Deserialize, Clone)]
pub struct PostJob {
    pub source_code: String,
    pub language: String,
    pub user_id: i32,
    pub contest_id: i32,
    pub problem_id: i32,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct CaseResult {
    id: i32,
    result: MyResult,
    time: i32,
    memory: i32,
    info: String,
}

impl CaseResult {
    fn new(id: i32) -> CaseResult {
        CaseResult {
            id,
            result: MyResult::Waiting,
            time: 0,
            memory: 0,
            info: "".to_string(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub enum State {
    Queueing,
    Running,
    Finished,
    Canceled,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct JobResponse {
    id: i32,
    created_time: String,
    updated_time: String,
    submission: PostJob,
    state: State,
    result: MyResult,
    score: f64,
    cases: Vec<CaseResult>,
}

impl JobResponse {
    fn new(post: &PostJob, problem: &Problem) -> JobResponse {
        JobResponse {
            id: 0,
            created_time: chrono::Local::now().to_string(),
            updated_time: chrono::Local::now().to_string(),
            submission: post.clone(),
            state: State::Queueing,
            result: MyResult::Waiting,
            score: 0.0,
            cases: {
                let mut count = 0;
                let mut cases: Vec<CaseResult> = vec![CaseResult::new(count)];
                for case in &problem.cases {
                    count += 1;
                    cases.push(CaseResult::new(count));
                }
                cases
            },
        }
    }
    fn update(&mut self) {
        self.updated_time = chrono::Local::now().to_string();
        self.state = State::Running;
    }
    fn final_result(&mut self) {
        self.updated_time = chrono::Local::now().to_string();
        self.state = State::Finished;
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub enum MyResult {
    Waiting,
    Running,
    Accepted,
    #[serde(rename = "Compilation Error")]
    CompilationError,
    #[serde(rename = "Compilation Success")]
    CompilationSuccess,
    #[serde(rename = "Wrong Answer")]
    WrongAnswer,
    #[serde(rename = "Runtime Error")]
    RuntimeError,
    #[serde(rename = "Time Limit Exceeded")]
    TimeLimitExceeded,
    #[serde(rename = "Memory Limit Exceeded")]
    MemoryLimitExceeded,
    #[serde(rename = "System Error")]
    SystemError,
    #[serde(rename = "SPJ Error")]
    SPJError,
    Skipped,
}

pub enum Reason {
    ErrInvalidArgument,
    ErrNotFound,
    ErrRateLimit,
    ErrExternal,
    ErrInternal,
}

pub fn get_language_problem(body: &Json<PostJob>, config: &Data<Config>) -> (Option<Language>, Option<Problem>) {
    let mut current_language: Option<Language> = None;
    let mut current_problem: Option<Problem> = None;


    //check and get problem and language
    let mut is_lan_in = false;
    for language in &config.languages {
        if language.name == body.language {
            current_language = Some(language.clone());
            is_lan_in = true;
        }
    }
    let mut is_prob_in = false;
    for problem in &config.problems {
        if problem.id == body.problem_id {
            current_problem = Some(problem.clone());
            is_prob_in = true;
        }
    }
    (current_language, current_problem)
}

pub fn run_job(current_language: &mut Language
               , problem: &mut Problem
               , body: &Json<PostJob>) -> Result<JobResponse, Reason> {
    let created_time = chrono::Local::now().to_string();
    let mut job_result: Option<MyResult> = None;
    let mut job_response = JobResponse::new(body, problem);

    //replace %INPUT% and %OUTPUT% of language
    let dir_path = format!("./problem{}", body.problem_id);
    let input_index = current_language.command.iter().position(|x| x == "%INPUT%").unwrap();
    let file_path = format!("{}/user_{}.rs", dir_path, body.user_id);
    current_language.command[input_index] = file_path.clone();
    let output_index = current_language.command.iter().position(|x| x == "%OUTPUT%").unwrap();
    let out_path = format!("{}/user_{}", dir_path, body.user_id);
    current_language.command[output_index] = out_path.clone();
    // println!("{:?}", current_language);

    //start to build
    create_dir(&dir_path).unwrap();
    fs::File::create(&file_path).unwrap();
    fs::write(file_path, &body.source_code).unwrap();
    let build_job = Command::new(&current_language.command[0])
        .args(&current_language.command[1..])
        .status().unwrap();
    if build_job.code() != Some(0) {
        job_result = Some(MyResult::CompilationError);
        job_response.cases[0].result = job_result.clone().unwrap();
        job_response.final_result();
        //if compile error, return.
    } else {
        //compile succeed
        job_response.cases[0].result = MyResult::CompilationSuccess;
        job_response.update();

        //case by case
        let mut score: f64 = 0.0;
        let mut case_id = 0;


        for case in &problem.cases {
            case_id += 1;
            let mut case_result = MyResult::Waiting;
            let mut run_case = Command::new(&out_path)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();

            run_case.stdin.take().unwrap()
                .write(fs::read_to_string(&case.input_file).unwrap().as_bytes())
                .unwrap();
            let time_limit = std::time::Duration::from_micros(case.time_limit as u64);

            //use time limit, get case result
            match run_case.wait_timeout(time_limit).unwrap() {
                None => {
                    run_case.kill().unwrap();
                    case_result = MyResult::TimeLimitExceeded;
                    job_response.update();
                }
                Some(s) => {
                    match s.code().unwrap() {
                        0 => {
                            //run successfully, match result
                            let mut out = run_case.stdout;
                            let mut output = String::new();
                            out.unwrap().read_to_string(&mut output).unwrap();
                            let ans = fs::read_to_string(&case.answer_file).unwrap();

                            let mut is_match = false;
                            match &problem.ty[..] {
                                "standard" => {
                                    let a: Vec<&str> = output.split("\n").map(|x| x.trim()).collect();
                                    let b: Vec<&str> = ans.split("\n").map(|x| x.trim()).collect();
                                    if a == b {
                                        is_match = true;
                                    }
                                }
                                "strict" => {
                                    if ans == output {
                                        is_match = true;
                                    }
                                }
                                _ => unimplemented!()
                            }

                            //got result, update response
                            if is_match {
                                job_response.score += case.score;
                                case_result = MyResult::Accepted;
                                job_response.update();
                            } else {
                                case_result = MyResult::WrongAnswer;
                                job_response.update();
                            }
                        }
                        a => {
                            println!("{:?}",a);
                            case_result = MyResult::RuntimeError;
                            job_response.update();
                        }
                    }
                }
            }
            match case_result {
                MyResult::Accepted => {}
                _ => {
                    if let None = job_result {
                        job_result = Some(case_result.clone())
                    }
                }
            }
            job_response.cases[case_id].result = case_result;
            job_response.update();
        }
    }
    fs::remove_dir_all(&dir_path).unwrap();
    if let Some(r) = job_result {
        job_response.result = r;
    }
    if job_response.score == 100.0 {
        job_response.result = MyResult::Accepted;
    }
    job_response.final_result();
    let a = serde_json::to_string_pretty(&job_response).unwrap();
    // println!("{}", a);
    Ok(job_response)
}


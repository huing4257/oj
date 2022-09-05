use actix_web;
use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_json;
use serde_json::Value;
use std::fs;
use std::fs::create_dir;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use actix_web::{HttpResponse, web};
use actix_web::web::Data;
use chrono::{DateTime, Utc};
use wait_timeout::ChildExt;


pub const TIME_FMT: &str = "%Y-%m-%dT%H:%M:%S%.3fZ";

///two args, parse by clap
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
pub struct Args {
    #[clap(short, long, value_parser)]
    pub config: String,
    #[clap(short, long = "flush-data")]
    pub flush_data: bool,
}

///address of server
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

///case data in a problem
#[derive(Serialize, Deserialize, Clone)]
pub struct Case {
    score: f64,
    input_file: String,
    answer_file: String,
    time_limit: i64,
    memory_limit: i64,
}

///a whole problem, give in config
#[derive(Serialize, Deserialize, Clone)]
pub struct Problem {
    pub id: i32,
    name: String,
    #[serde(rename = "type")]
    pub ty: String,
    misc: Option<Value>,
    pub cases: Vec<Case>,
}

///code language, also contains commands to build a program
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Language {
    pub name: String,
    file_name: String,
    pub command: Vec<String>,
}

///config of whole oj
#[derive(Serialize, Deserialize, Clone)]
pub struct Config {
    server: Server,
    pub problems: Vec<Problem>,
    pub languages: Vec<Language>,
}

/// a post job from a client, contains all information of how to deal with the job
#[derive(Serialize, Deserialize, Clone)]
pub struct PostJob {
    pub source_code: String,
    pub language: String,
    pub user_id: i32,
    pub contest_id: i32,
    pub problem_id: i32,
}

///the filter requirements of service get_jobs
#[derive(Serialize, Deserialize, Clone)]
pub struct GetJob {
    user_id: Option<i32>,
    user_name: Option<String>,
    contest_id: Option<i32>,
    problem_id: Option<i32>,
    language: Option<String>,
    from: Option<String>,
    to: Option<String>,
    state: Option<State>,
    result: Option<MyResult>,
}

///the test result of a single result
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

///the test state of a whole post job
#[derive(Serialize, Deserialize, Clone,Eq, PartialEq)]
pub enum State {
    Queueing,
    Running,
    Finished,
    Canceled,
}

///the test result of a whole job
#[derive(Serialize, Deserialize, Clone)]
pub struct Job {
    pub id: i32,
    created_time: String,
    updated_time: String,
    pub submission: PostJob,
    state: State,
    result: MyResult,
    score: f64,
    cases: Vec<CaseResult>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Error{
    pub reason:Reason,
    pub code:i32,
    pub message:String
}
///get formatted current time
fn my_now() -> String {
    Utc::now().format(TIME_FMT).to_string()
}

impl Job {
    pub fn new(id:i32,post: &PostJob) -> Job {
        Job {
            id,
            created_time: my_now(),
            updated_time: my_now(),
            submission: post.clone(),
            state: State::Queueing,
            result: MyResult::Waiting,
            score: 0.0,
            cases: vec![],
        }
    }
    ///refresh a job's updated time
    fn update(&mut self) {
        self.updated_time = my_now();
        self.state = State::Running;
    }
    ///refresh a job's updated time, and set state finished
    fn final_result(&mut self) {
        self.updated_time = my_now();
        self.state = State::Finished;
    }
}

///All possible result of a job or a case
#[derive(Serialize, Deserialize, Clone, Eq, PartialEq)]
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

/// reasons why a request failed
#[derive(Serialize, Deserialize, Clone)]
pub enum Reason {
    ErrInvalidArgument,
    #[serde(rename="ERR_NOT_FOUND")]
    ErrNotFound,
    ErrRateLimit,
    ErrExternal,
    ErrInternal,
}

///use language, problem information, run a post job update job_list
pub fn run_job(
    job:&mut Job,
    config: &web::Data<Config>,
) -> Result<(), Error> {
    let mut current_language: Option<Language> = None;
    let mut problem: Option<Problem> = None;

    //initialize job

    //check and get problem and language
    for language in &config.languages {
        if language.name == job.submission.language {
            current_language = Some(language.clone());
        }
    }
    for problem0 in &config.problems {
        if problem0.id == job.submission.problem_id {
            problem = Some(problem0.clone());
        }
    }
    if current_language.is_none() || problem.is_none() {
        return Err(Error{
            reason: Reason::ErrNotFound,
            code: 3,
            message: "".to_string()
        });
    }
    let mut current_language=current_language.unwrap();
    let problem=problem.unwrap();
    let mut job_result: Option<MyResult> = None;

    //initialize job cases, clear and push default
    {
        job.result=MyResult::Waiting;
        job.score=0.0;
        job.cases.clear();
        let mut count = 0;
        job.cases.push(CaseResult::new(count));
        for _case in &problem.cases {
            count += 1;
            job.cases.push(CaseResult::new(count));
        }
    }

    //replace %INPUT% and %OUTPUT% of language
    let dir_path = format!("./problem{}", job.submission.problem_id);
    let input_index = current_language
        .command
        .iter()
        .position(|x| x == "%INPUT%")
        .unwrap();
    let file_path = format!("{}/{}", dir_path, current_language.file_name);
    current_language.command[input_index] = file_path.clone();
    let output_index = current_language
        .command
        .iter()
        .position(|x| x == "%OUTPUT%")
        .unwrap();
    let out_path = format!("{}/job_{}", dir_path, job.submission.user_id);
    current_language.command[output_index] = out_path.clone();
    // println!("{:?}", current_language);

    //start to compile
    create_dir(&dir_path).unwrap();
    fs::File::create(&file_path).unwrap();
    fs::write(file_path, &job.submission.source_code).unwrap();
    let build_job = Command::new(&current_language.command[0])
        .args(&current_language.command[1..])
        .status()
        .unwrap();

    if build_job.code() != Some(0) {
        job_result=Some(MyResult::CompilationError);

        job.cases[0].result=MyResult::CompilationError;
        job.final_result();
        //if compile error, return.
    } else {
        //compile succeed
        job.cases[0].result=MyResult::CompilationSuccess;
        job.update();

        //case by case
        let mut case_id = 0;
        for case in &problem.cases {
            case_id += 1;
            //var to record result
            let case_result: MyResult;
            let mut run_case = Command::new(&out_path)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            run_case
                .stdin
                .take()
                .unwrap()
                .write(fs::read_to_string(&case.input_file).unwrap().as_bytes())
                .unwrap();
            let time_limit = std::time::Duration::from_micros(case.time_limit as u64);

            //use time limit, get case result
            match run_case.wait_timeout(time_limit).unwrap() {
                None => {
                    run_case.kill().unwrap();
                    case_result = MyResult::TimeLimitExceeded;
                    job.update();
                }
                Some(s) => {
                    match s.code().unwrap() {
                        0 => {
                            //run successfully, match result
                            let out = run_case.stdout;
                            let mut output = String::new();
                            out.unwrap().read_to_string(&mut output).unwrap();
                            let ans = fs::read_to_string(&case.answer_file).unwrap();

                            let mut is_match = false;
                            match &problem.ty[..] {
                                "standard" => {
                                    let a: Vec<&str> =
                                        output.split("\n").map(|x| x.trim()).collect();
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
                                _ => unimplemented!(),
                            }

                            //got result, update response
                            if is_match {
                                job.score += case.score;
                                case_result = MyResult::Accepted;
                                job.update();
                            } else {
                                case_result = MyResult::WrongAnswer;
                                job.update();
                            }
                        }
                        a => {
                            println!("{:?}", a);
                            case_result = MyResult::RuntimeError;
                            job.update();
                        }
                    }
                }
            }
            //handle case result
            match case_result {
                MyResult::Accepted => {
                }
                _ => {
                    if let None = job_result {
                        job_result = Some(case_result.clone())
                    }
                }
            }
            job.cases[case_id].result=case_result;
            job.update();
        }
    }
    fs::remove_dir_all(&dir_path).unwrap();
    if let Some(r) = job_result {
        job.result = r;
    }
    if job.score == 100.0 {
        job.result = MyResult::Accepted;
    }
    job.final_result();
    // let a = serde_json::to_string_pretty(&job).unwrap();
    // println!("{}", a);
    Ok(())
}

pub fn match_job(require: &GetJob, job:&Job) -> bool {
    //any option unsatisfied, return false
    if let Some(parameter) = &require.result {
        if job.result != *parameter {
            return false;
        }
    }
    if let Some(parameter) = &require.from {
        let from_time = DateTime::parse_from_str(&parameter, TIME_FMT).unwrap();
        let actual_time = DateTime::parse_from_str(&job.created_time, TIME_FMT).unwrap();
        if from_time >= actual_time {
            return false;
        }
    }
    if let Some(parameter) = &require.to {
        let to_time = DateTime::parse_from_str(&parameter, TIME_FMT).unwrap();
        let actual_time = DateTime::parse_from_str(&job.created_time, TIME_FMT).unwrap();
        if to_time <= actual_time {
            return false;
        }
    }
    if let Some(parameter) = &require.language {
        if parameter!=&job.submission.language {
            return false
        }
    }
    if let Some(parameter) = &require.state {
        if parameter!=&job.state {
            return false;
        }
    }
    if let Some(parameter) = &require.problem_id {
        if parameter != &job.submission.problem_id {
            return false;
        }
    }
    // if let Some(parameter)=&require.contest_id{
    //
    // }
    //
    // if let Some(parameter)=&require.user_id{
    //
    // }
    // if let Some(parameter)=&require.user_name{
    //
    // }
    true
}
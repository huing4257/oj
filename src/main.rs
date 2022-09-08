use actix_web;
use actix_web::rt::spawn;
use actix_web::{
    get, middleware::Logger, post, put, web, App, HttpResponse, HttpServer, Responder,
};
use clap;
use clap::Parser;
use env_logger;
use lazy_static::lazy_static;
use log;
use oj;
use oj::{compare_users, get_score_list, get_user_submissions, match_job, run_job,
         Config, Job, PostJob, Reason, User, UserRank, Contest};
use std::cmp::Ordering;
use std::fs;
use std::ops::Deref;
use std::sync::{Arc, Mutex};
use chrono::{FixedOffset};
lazy_static! {
    static ref JOB_LIST: Arc<Mutex<Vec<Job>>> = Arc::new(Mutex::new(Vec::new()));
}
lazy_static! {
    static ref UESR_LIST: Arc<Mutex<Vec<User>>> = Arc::new(Mutex::new(vec![]));
}
lazy_static! {
    static ref CONTEST_LIST: Arc<Mutex<Vec<Contest>>> = Arc::new(Mutex::new(vec![]));
}
#[get("/hello/{name}")]
async fn greet(name: web::Path<String>) -> impl Responder {
    log::info!(target: "greet_handler", "Greeting {}", *name);
    format!("Hello {name}!")
}

// DO NOT REMOVE: used in automatic testing
#[post("/internal/exit")]
#[allow(unreachable_code)]
async fn exit() -> impl Responder {
    save_data();
    log::info!("Shutdown as requested");
    std::process::exit(0);
    format!("Exited")
}

#[post("/jobs")]
async fn post_jobs(body: web::Json<PostJob>, config: web::Data<Config>) -> impl Responder {
    let mut lock = JOB_LIST.lock().unwrap();
    let job_list = lock.clone();
    let contest_list = CONTEST_LIST.lock().unwrap().to_vec();
    let id = lock.len();
    //create a job
    let mut job = Job::new(id as i32, &body.0);
    //if user doesn't exist, return error
    if job.submission.user_id >= UESR_LIST.lock().unwrap().len() as i32 {
        HttpResponse::NotFound().json(oj::Error {
            reason: Reason::ErrNotFound,
            code: 3,
            message: "User id not found".to_string(),
        })
    } else {
        let result = web::block(move || {
            run_job(&mut job, &config, &contest_list, job_list)
        }).await;

        match result.unwrap() {
            Ok(job) => {
                //push modified job
                lock.push(job.clone());
                HttpResponse::Ok().json(job)
            }
            Err(err) => err.to_response()
        }
    }
}

#[get("/jobs")]
async fn get_jobs(query: web::Query<oj::GetJob>) -> impl Responder {
    let mut return_list: Vec<Job> = vec![];
    for i in &*JOB_LIST.lock().unwrap() {
        if match_job(&query, i, UESR_LIST.lock().unwrap().as_ref()) {
            return_list.push(i.clone());
        }
    }
    return HttpResponse::Ok().json(return_list);
}

#[get("/jobs/{job_id}")]
async fn get_job(job_id: web::Path<i32>) -> impl Responder {
    let id: i32 = job_id.into_inner();
    let job = JOB_LIST.lock().unwrap().iter().find(|x| x.id == id).cloned();
    return match job {
        Some(a) => HttpResponse::Ok().json(a),
        None => HttpResponse::NotFound()
            .json("{ reason=ERR_NOT_FOUND, code=3, message=\"Job 123456 not found.\"}"),
    };
}

#[put("/jobs/{job_id}")]
async fn put_job(job_id: web::Path<i32>, config: web::Data<Config>) -> impl Responder {
    let contest_list = CONTEST_LIST.lock().unwrap().to_vec();
    let mut lock = JOB_LIST.lock().unwrap();
    let id: i32 = job_id.into_inner();
    let job_list = lock.clone();
    let job = lock.iter_mut().find(|x| x.id == id);
    if let None = job {
        return HttpResponse::NotFound().json(oj::Error {
            reason: Reason::ErrNotFound,
            code: 3,
            message: "Job 123456 not found.".to_string(),
        });
    }
    let mut job = job.unwrap();
    match run_job(&mut job, &config, &contest_list, job_list) {
        Ok(job_response) => {
            HttpResponse::Ok().json(job_response)
        }
        Err(err) => {
            err.to_response()
        }
    }
}

#[get("/users")]
async fn get_users() -> impl Responder {
    HttpResponse::Ok().json(UESR_LIST.lock().unwrap().to_vec())
}

#[post("/users")]
async fn post_users(user: web::Json<User>) -> impl Responder {
    let mut user = user.deref().clone();
    let mut is_name_in = false;
    let mut is_id_in = false;
    for i in UESR_LIST.lock().unwrap().iter_mut() {
        //update name
        if user.id == i.id {
            i.name = user.name.clone();
            is_id_in = true;
            break;
        }
        if user.name == i.name {
            is_name_in = true;
        }
    }
    return if is_name_in {
        //name in, error directly
        HttpResponse::BadRequest().json(oj::Error {
            reason: Reason::ErrInvalidArgument,
            code: 1,
            message: format!("User name '{}' already exists.", user.name),
        })
    } else if user.id.is_none() {
        //name not corrupt, and didn't appoint id
        user.id = Some(UESR_LIST.lock().unwrap().len() as i32);
        UESR_LIST.lock().unwrap().push(user.clone());
        CONTEST_LIST.lock().unwrap()[0].user_ids.push(user.id.unwrap());
        HttpResponse::Ok().json(user)
    } else if !is_id_in {
        //appointed id doesn't exist
        HttpResponse::NotFound().json(oj::Error {
            reason: Reason::ErrNotFound,
            code: 3,
            message: format!("User {} already exists.", user.id.unwrap()),
        })
    } else {
        HttpResponse::Ok().json(user)
    };
}

#[post("/contests")]
async fn post_contest(body: web::Json<Contest>, config: web::Data<Config>) -> impl Responder {
    let user_list = UESR_LIST.lock().unwrap().to_vec();
    for user_id in &body.user_ids {
        if user_list.iter().map(|x| x.id).position(|x| x.unwrap() == *user_id).is_none() {
            return HttpResponse::NotFound().json(oj::Error {
                reason: Reason::ErrNotFound,
                code: 3,
                message: format!("user {} not found", user_id),
            });
        }
    }
    for problem_id in &body.problem_ids {
        if config.problems.iter().map(|x| x.id).position(|x| x == *problem_id).is_none() {
            return HttpResponse::NotFound().json(oj::Error {
                reason: Reason::ErrNotFound,
                code: 3,
                message: format!("problem{} not found", problem_id),
            });
        }
    }
    let mut contest = body.into_inner();
    let mut contest_list = CONTEST_LIST.lock().unwrap();
    return if contest.id.is_none() {
        contest.id = Some(contest_list.len() as i32);
        contest_list.push(contest.clone());
        HttpResponse::Ok().json(contest)
    } else {
        match contest_list.iter().map(|x| x.id).position(|x| x == contest.id) {
            None => {
                HttpResponse::NotFound().json(oj::Error {
                    reason: Reason::ErrNotFound,
                    code: 3,
                    message: format!("contest{} not found", contest.id.unwrap()),
                })
            }
            Some(index) => {
                contest_list[index] = contest.clone();
                HttpResponse::Ok().json(contest)
            }
        }
    };
}

#[get("/contests")]
async fn get_contests() -> impl Responder {
    return HttpResponse::Ok().json(
        {
            let list: Vec<Contest> = CONTEST_LIST.lock().unwrap().to_vec().iter()
                .filter(|x| x.id.unwrap() != 0)
                .cloned().collect();
            list
        }
    );
}

#[get("/contests/{contest_id}")]
async fn get_contest(contest_id: web::Path<i32>) -> impl Responder {
    let contest_id = contest_id.into_inner();
    let contest = CONTEST_LIST.lock().unwrap()
        .to_vec().iter()
        .find(|x| x.id.unwrap() == contest_id).cloned();
    match contest {
        None => {
            HttpResponse::NotFound().json(
                oj::Error {
                    reason: Reason::ErrNotFound,
                    code: 3,
                    message: "".to_string(),
                }
            )
        }
        Some(c) => {
            HttpResponse::Ok().json(c)
        }
    }
}

#[get("/contests/{contest_id}/ranklist")]
async fn get_rank_list(
    contest_id: web::Path<i32>,
    rule: web::Query<oj::RankRule>,
    config: web::Data<Config>,
) -> impl Responder {
    //get useful data
    let contest_id = contest_id.into_inner();
    let user_list: Vec<User> = UESR_LIST.lock().unwrap().deref().to_vec();
    let contest = CONTEST_LIST.lock().unwrap().iter().find(|x| x.id.unwrap() == contest_id).cloned();
    //if contest didn't found return error
    if contest.is_none() {
        return HttpResponse::NotFound().json(
            oj::Error {
                reason: Reason::ErrNotFound,
                code: 3,
                message: format!("contest{} not found", contest_id),
            }
        );
    }
    // contest exists, unwrap, and use it to filter users
    let contest = contest.unwrap();
    let mut user_list: Vec<User> = user_list.iter().filter(|x| contest.user_ids.contains(&x.id.unwrap())).cloned().collect();
    let rule = rule.deref();
    let job_list = JOB_LIST.lock().unwrap().deref().to_vec();

    //sort users by scoring rule at first, and default use id as tie breaker
    user_list.sort_by(|a, b| {
        let a_list = get_user_submissions(contest_id, a, &job_list);
        let b_list = get_user_submissions(contest_id, b, &job_list);
        let a_tuple = get_score_list(&contest, &job_list, &a_list, rule, config.deref());
        let a_score: f64 = a_tuple.0.iter().sum();
        let b_tuple = get_score_list(&contest, &job_list, &b_list, rule, config.deref());
        let b_score: f64 = b_tuple.0.iter().sum();
        let order = compare_users(
            &b_list,
            &a_list,
            (b_score, a_score),
            (b_tuple.1, a_tuple.1),
            rule,
        );
        if let Ordering::Equal = order {
            a.id.cmp(&b.id)
        } else {
            order
        }
    });

    let mut rank: Vec<UserRank> = vec![];
    let list = get_user_submissions(contest_id, &user_list[0], &job_list);
    let scores = get_score_list(&contest, &job_list, &list, rule, config.deref()).0;
    rank.push(UserRank {
        user: user_list[0].clone(),
        rank: 1,
        scores,
    });
    for i in 1..user_list.len() {
        let former_list = get_user_submissions(contest_id, &user_list[i - 1], &job_list);
        let now_list = get_user_submissions(contest_id, &user_list[i], &job_list);
        let f = get_score_list(&contest, &job_list, &former_list, rule, config.deref());
        let f_score: f64 = f.0.iter().sum();
        let f_index = f.1;
        let n = get_score_list(&contest, &job_list, &now_list, rule, config.deref());
        let n_score: f64 = n.0.iter().sum();
        let n_index = n.1;
        let scores = get_score_list(&contest, &job_list, &now_list, rule, config.deref()).0;

        if Ordering::Equal == compare_users(
            &former_list,
            &now_list,
            (f_score, n_score),
            (f_index, n_index),
            rule,
        ) {
            rank.push(UserRank {
                user: user_list[i].clone(),
                rank: rank[i - 1].rank,
                scores,
            });
        } else {
            rank.push(UserRank {
                user: user_list[i].clone(),
                rank: (i + 1) as i32,
                scores,
            });
        }
    }
    HttpResponse::Ok().json(rank)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let args = oj::Args::parse();
    let path = args.config;
    let is_flush = args.flush_data;
    let config: Config = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    if is_flush {
        UESR_LIST.lock().unwrap().push(User {
            id: Some(0),
            name: "root".to_string(),
        });
        CONTEST_LIST.lock().unwrap().push(
            Contest {
                id: Some(0),
                name: "".to_string(),
                from: {
                    let time: chrono::DateTime<FixedOffset> = chrono::DateTime::default();
                    time.to_string()
                },
                to: {
                    let time: chrono::NaiveDateTime = chrono::NaiveDateTime::MAX;
                    time.to_string()
                },
                problem_ids: vec![],
                user_ids: vec![0],
                submission_limit: 0,
            }
        );
    } else {
        read_data();
    }
    spawn(async {
        let mut interval = actix_web::rt::time::interval(std::time::Duration::from_micros(500000));
        loop {
            interval.tick().await;
            save_data();
        }
    });
    // save_task.
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));
    HttpServer::new(move || {
        App::new()
            .wrap(Logger::default())
            .app_data(web::Data::new(config.clone()))
            .route("/hello", web::get().to(|| async { "Hello World!" }))
            .service(greet)
            .service(post_jobs)
            .service(get_jobs)
            .service(get_job)
            .service(put_job)
            .service(post_users)
            .service(get_users)
            .service(get_rank_list)
            .service(post_contest)
            .service(get_contest)
            .service(get_contests)
            // DO NOT REMOVE: used in automatic testing
            .service(exit)
    })
        .bind(("127.0.0.1", 12345))?
        .run()
        .await
}

fn read_data() {
    let mut job_lock = JOB_LIST.lock().unwrap();
    let jobs_string = fs::read_to_string("./jobs.json").unwrap();
    let initial_jobs: Vec<Job> = serde_json::from_str(&jobs_string).unwrap();
    *job_lock = initial_jobs;
    let mut user_lock = UESR_LIST.lock().unwrap();
    let user_string = fs::read_to_string("./users.json").unwrap();
    let initial_users: Vec<User> = serde_json::from_str(&user_string).unwrap();
    *user_lock = initial_users;
    let mut contest_lock = CONTEST_LIST.lock().unwrap();
    let contest_string = fs::read_to_string("./contests.json").unwrap();
    let initial_contests: Vec<Contest> = serde_json::from_str(&contest_string).unwrap();
    *contest_lock = initial_contests;
}

fn save_data() {
    let jobs_lock = JOB_LIST.lock().unwrap();
    let jobs: String = serde_json::to_string_pretty(&*jobs_lock).unwrap();
    fs::write("./jobs.json", jobs).unwrap();
    let users_lock = UESR_LIST.lock().unwrap();
    let users: String = serde_json::to_string_pretty(&*users_lock).unwrap();
    fs::write("./users.json", users).unwrap();
    let contests_lock = CONTEST_LIST.lock().unwrap();
    let contests: String = serde_json::to_string_pretty(&*contests_lock).unwrap();
    fs::write("./contests.json", contests).unwrap();
}

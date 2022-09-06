use std::borrow::{BorrowMut};
use std::cmp::Ordering;
use actix_web::{get, put, middleware::Logger, post, web, App, HttpResponse, HttpServer, Responder};
use clap::Parser;
use env_logger;
use log;
use oj;
use oj::{Config, PostJob, run_job, Job, match_job, Reason, User, get_user_submissions, compare_users, UserRank, get_score_list};
use std::fs;
use std::ops::{Deref};
use actix_web;
use std::sync::{Arc, Mutex};
use actix_web::web::{Path, Query};
use lazy_static::lazy_static;
lazy_static! {
    static ref JOB_LIST: Arc<Mutex<Vec<Job >>> = Arc::new(Mutex::new(Vec::new()));
}
lazy_static! {
    static ref UESR_LIST:Arc<Mutex<Vec<User>>> = Arc::new(Mutex::new(vec![]));
}
#[get("/hello/{name}")]
async fn greet(name: web::Path<String>) -> impl Responder {
    log::info!(target: "greet_handler", "Greeting {}", name);
    format!("Hello {name}!")
}

// DO NOT REMOVE: used in automatic testing
#[post("/internal/exit")]
#[allow(unreachable_code)]
async fn exit() -> impl Responder {
    log::info!("Shutdown as requested");
    std::process::exit(0);
    format!("Exited")
}

#[post("/jobs")]
async fn post_jobs(body: web::Json<PostJob>, config: web::Data<Config>) -> impl Responder {
    println!("0");
    let response: HttpResponse;
    let mut lock = JOB_LIST.lock().unwrap();
    let id = lock.len();
    //create a job
    let mut job = Job::new(id as i32, &body.0);
    if job.submission.user_id >= UESR_LIST.lock().unwrap().len() as i32 {
        return HttpResponse::NotFound().json(oj::Error {
            reason: Reason::ErrNotFound,
            code: 3,
            message: "".to_string(),
        });
    }
    match run_job(&mut job, &config) {
        Ok(_) => {
            response = HttpResponse::Ok().json(&job)
        }
        Err(err) => {
            response = HttpResponse::NotFound().json(err)
        }
    };
    //push modified job
    lock.push(job);
    response
}


#[get("/jobs")]
async fn get_jobs(body: web::Query<oj::GetJob>) -> impl Responder {
    let mut return_list: Vec<Job> = vec![];
    for i in &*JOB_LIST.lock().unwrap() {
        if match_job(&body, i, UESR_LIST.lock().unwrap().as_ref()) {
            return_list.push(i.clone());
        }
    }
    println!("1");
    return HttpResponse::Ok().json(return_list);
}

#[get("/jobs/{job_id}")]
async fn get_job(job_id: Path<i32>) -> impl Responder {
    let mut job: Option<Job> = None;
    let id: i32 = job_id.into_inner();
    for i in &*JOB_LIST.lock().unwrap() {
        if i.id == id {
            job = Some(i.clone());
        }
    }
    return match job {
        Some(a) => {
            HttpResponse::Ok().json(a)
        }
        None => {
            HttpResponse::NotFound().json("{ reason=ERR_NOT_FOUND, code=3, message=\"Job 123456 not found.\"}")
        }
    };
}

#[put("/jobs/{job_id}")]
async fn put_job(job_id: Path<i32>, config: web::Data<Config>) -> impl Responder {
    let mut job: Option<&mut Job> = None;
    let id: i32 = job_id.into_inner();
    let mut lock = JOB_LIST.lock().unwrap();
    for i in lock.borrow_mut().iter_mut() {
        if i.id == id {
            job = Some(i);
        }
    }
    if let None = job {
        return HttpResponse::NotFound().json(oj::Error {
            reason: Reason::ErrNotFound,
            code: 3,
            message: "Job 123456 not found.".to_string(),
        });
    }
    let mut job = job.unwrap();
    match run_job(&mut job, &config) {
        Ok(job_response) => {
            HttpResponse::Ok().json(job_response);
        }
        Err(err) => {
            HttpResponse::NotFound().json(err);
        }
    }
    HttpResponse::Ok().json(job.clone())
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
        if user.id == i.id {
            i.name = user.name.clone();
            is_id_in = true;
            break;
        }
        if user.name == i.name {
            is_name_in = true;
        }
    }
    //name in, error directly
    return if is_name_in {
        HttpResponse::BadRequest().json(oj::Error {
            reason: Reason::ErrInvalidArgument,
            code: 1,
            message: format!("User name '{}' already exists.", user.name),
        })
    } else if user.id.is_none() {
        user.id = Some(UESR_LIST.lock().unwrap().len() as i32);
        UESR_LIST.lock().unwrap().push(user.clone());
        HttpResponse::Ok().json(user)
    } else if !is_id_in {
        HttpResponse::NotFound().json(oj::Error {
            reason: Reason::ErrNotFound,
            code: 3,
            message: format!("User {} already exists.", user.id.unwrap()),
        })
    } else {
        HttpResponse::Ok().json(user)
    };
}

#[get("/contests/{contest_id}/ranklist")]
async fn get_rank_list(contest_id: web::Path<i32>, rule: Query<oj::RankRule>, config: web::Data<Config>) -> impl Responder {
    let mut user_list: Vec<User> = UESR_LIST.lock().unwrap().deref().clone();
    let rule = rule.deref();
    let job_list = JOB_LIST.lock().unwrap().deref().to_vec();
    user_list.sort_by(|a, b| {
        let a_list = get_user_submissions(a, &job_list);
        let b_list = get_user_submissions(b, &job_list);
        let a_score: f64 = get_score_list(&a_list, rule, config.deref()).iter().sum();
        let b_score: f64 = get_score_list(&b_list, rule, config.deref()).iter().sum();
        if let Ordering::Equal = compare_users(&b_list, &a_list, (b_score, a_score), rule) {
            a.id.cmp(&b.id)
        } else {
            compare_users(&b_list, &a_list, (b_score, a_score), rule)
        }
    });
    let mut rank: Vec<UserRank> = vec![];

    let list = get_user_submissions(&user_list[0], &job_list);
    let scores = get_score_list(&list, rule, config.deref());
    rank.push(UserRank {
        user: user_list[0].clone(),
        rank: 1,
        scores,
    });
    for i in 1..user_list.len() {
        let former_list = get_user_submissions(&user_list[i - 1], &job_list);
        let now_list = get_user_submissions(&user_list[i], &job_list);
        let f_score: f64 = get_score_list(&former_list, rule, config.deref()).iter().sum();
        let n_score: f64 = get_score_list(&now_list, rule, config.deref()).iter().sum();
        let scores = get_score_list(&now_list, rule, config.deref());

        if Ordering::Equal == compare_users(&former_list, &now_list, (f_score, n_score), rule) {
            if !former_list.is_empty() &&!now_list.is_empty()
            { println!("{},{}", former_list[0].submission.user_id, now_list[0].submission.user_id); }
            rank.push(
                UserRank {
                user: user_list[i].clone(),
                rank: rank[i - 1].rank,
                scores,
            });
        } else {
            rank.push(
                UserRank {
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
    UESR_LIST.lock().unwrap().push(User {
        id: Some(0),
        name: "root".to_string(),
    });
    let path = oj::Args::parse().config;
    let config: Config = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
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
            // DO NOT REMOVE: used in automatic testing
            .service(exit)
    })
        .bind(("127.0.0.1", 12345))?
        .run()
        .await
}

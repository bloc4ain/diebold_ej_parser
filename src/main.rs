extern crate regex;
use regex::Regex;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::fs::File;
use std::io;
use std::io::{BufRead, BufReader, Read, Write};
use std::panic;
use std::path::Path;
use std::vec::Vec;

const TXN_SEP_REGEX: &str = r"\*{55}";
const SUCCESS_CWD_REGEX: &str = r" RESP CODE : 00 \r*\n TRN TYPE  : (CASH WITHDRAWAL|FAST CASH)";
const FILE_NAME_REGEX: &str = r"(\d{8})-(\d{4}-\d{1,2}-\d{1,2}).txt$";
const TRANSACTION_RANGE: i32 = 3;

struct JournalFile {
    terminal_id: String,
    date_time: String,
    path: String,
}

struct CardSession {
    data: String,
    complete: bool,
    successful_cwd: bool,
}

fn read_trace() -> String {
    let stdin = io::stdin();
    loop {
        println!("Enter trace number: ");
        let trace = stdin.lock().lines().next().unwrap().unwrap();
        if trace.is_empty() {
            println!("Empty trace entered!");
            continue;
        }
        if !trace.parse::<i32>().is_ok() {
            println!("Please enter a valid number!");
            continue;
        }
        return trace;
    }
}

fn read_path(msg: &str) -> String {
    let stdin = io::stdin();
    loop {
        println!("{}", msg);
        let path = stdin.lock().lines().next().unwrap().unwrap();
        if path.is_empty() {
            let current = env::current_dir().unwrap();
            let lossy = current.to_string_lossy().to_string();
            println!("Using current dir: {}", lossy);
            return lossy;
        }
        if !Path::new(&path).exists() {
            println!("Directory does not exist!");
            continue;
        }
        return path;
    }
}

fn get_journal_files(path: String) -> Vec<JournalFile> {
    let paths = fs::read_dir(path).unwrap();
    let re = Regex::new(FILE_NAME_REGEX).unwrap();
    let mut files = Vec::new();
    for path in paths {
        let dir_entry = path.unwrap();
        let file_name = dir_entry.file_name().into_string().unwrap();
        match re.captures(&file_name) {
            Some(cap) => files.push(JournalFile {
                terminal_id: String::from(&cap[1]),
                date_time: String::from(&cap[2]),
                path: dir_entry.path().into_os_string().into_string().unwrap(),
            }),
            None => {}
        }
    }
    files
}

fn validate_journal_files(files: &Vec<JournalFile>) -> bool {
    let mut hm = HashMap::new();
    for file in files {
        hm.insert(file.terminal_id.clone(), true);
        if hm.len() > 1 {
            return false;
        }
    }
    return true;
}

fn save_file(content: &str) {
    let dir = read_path("Enter directory where to save output (leave empty for current folder): ");
    let filename = Path::new(&dir).join("output.txt");
    if filename.exists() {
        println!("File already exists... please select another file");
    } else {
        let mut file = File::create(filename.to_str().unwrap()).unwrap();
        file.write_all(content.as_bytes()).unwrap();
        println!("Output saved to {}", filename.to_string_lossy());
    }
}

fn pause() {
    io::stdin().read(&mut [0]).unwrap();
}

fn find() {
    let trace = read_trace();
    let path = read_path(
        "Enter directory name containing journal logs (leave empty for current folder): ",
    );
    let mut files = get_journal_files(String::from(path));

    if files.is_empty() {
        println!("Could not find electronic journal files in the directory!");
        pause();
        return;
    }

    // The folder must contain files from one terminal only
    // Managing multiple terminals is complicated and requires more advanced logic
    if !validate_journal_files(&files) {
        println!("Found journal logs for more than one terminal. Aborting.");
        println!("Ensure that the folder contains files from one terminal to prevent errors!");
        pause();
        return;
    }

    // We need the files ordered by date so we can traverse them line by line
    // and be sure that if transaction is incomplete in the end of a file
    // we will find the rest of it in the next file (if exist)
    files.sort_by(|a, b| a.date_time.cmp(&b.date_time));

    // Merge all files and treat them as one stream of lines
    let lines = files
        .iter()
        .map(|file| File::open(file.path.clone()).unwrap())
        .flat_map(|file| BufReader::new(file).lines())
        .map(|line| line.unwrap());

    let card_session_sep_re = Regex::new(TXN_SEP_REGEX).unwrap();
    let txn_success_cwd_re = Regex::new(SUCCESS_CWD_REGEX).unwrap();
    let txn_trace_re = Regex::new(&format!(" TRACE     : {}", trace)).unwrap();
    let mut card_sessions = Vec::new();
    let mut card_session_parts = Vec::new();
    let mut card_session_started = false;
    let mut txn_found = false;
    let mut successful_cwd_before = 0;
    let mut successful_cwd_after = 0;

    for line in lines {
        // If we did not hit start/end of card session we just push the line to the queue and
        // continue collecting data
        if !card_session_sep_re.is_match(&line) {
            card_session_parts.push(line);
            continue;
        }

        // Having no data collected means that we are in beginning of a session so
        // we ignore it and continue collecting
        if card_session_parts.is_empty() {
            card_session_started = true;
            continue;
        }

        // At this point we are at the end of a card session
        // so we can proceed with our checks
        let card_session = card_session_parts.join("\n");
        // If we did not hit session separator yet,
        // we cannot guarantee that the session is complete
        let complete = card_session_started;
        // We need to retrieve 3 card sessions that contain successful cash withdrawal
        // so we test if the current session contains such
        let successful_cwd = txn_success_cwd_re.is_match(&card_session);

        if !txn_found && txn_trace_re.is_match(&card_session) {
            // The next statement will increment the 'after' counter
            // We don't want to count the match txn as 'after'
            successful_cwd_after -= 1;
            txn_found = true;
        }
        if !txn_found && successful_cwd {
            successful_cwd_before += 1;
        }
        if txn_found && successful_cwd {
            successful_cwd_after += 1;
        }

        card_sessions.push(CardSession {
            data: card_session,
            complete: complete,
            successful_cwd: successful_cwd,
        });
        card_session_started = true;
        card_session_parts.clear();

        if txn_found && successful_cwd_after >= TRANSACTION_RANGE {
            break;
        }
        if !txn_found && successful_cwd_before > TRANSACTION_RANGE {
            // Cleanup unecesarry card sessions to save memory
            while successful_cwd_before > TRANSACTION_RANGE {
                if card_sessions.remove(0).successful_cwd {
                    successful_cwd_before -= 1;
                }
            }
        }
    }

    if !txn_found {
        println!("Transaction with trace {} not found!", trace);
        pause();
        return;
    }

    println!("Found transaction with trace {}", trace);

    if successful_cwd_before < TRANSACTION_RANGE || !card_sessions.first().unwrap().complete {
        println!(
            "Unable to take {} previous successful cash withdrawals, have {}",
            TRANSACTION_RANGE, successful_cwd_before
        );
        println!(
            "Please include in the directory file for the day before {}",
            files.first().unwrap().date_time
        );
    } else if successful_cwd_after < TRANSACTION_RANGE || !card_sessions.last().unwrap().complete {
        println!(
            "Unable to take {} later successful cash withdrawals",
            TRANSACTION_RANGE
        );
        println!(
            "Please include in the directory file for the day after {}",
            files.last().unwrap().date_time
        );
    } else {
        let mut output = String::new();
        output.push_str("*******************************************************\n");
        output.push_str(
            &card_sessions
                .iter()
                .map(|cs| cs.data.clone())
                .collect::<Vec<String>>()
                .join("\n*******************************************************\n"),
        );
        output.push_str("\n*******************************************************");
        save_file(&output);
    }
    pause();
}

fn main() {
    let result = panic::catch_unwind(find);
    if result.is_err() {
        println!("Fatal error occured. Please contact the application developer.");
        println!("{:?}", result);
    }
}

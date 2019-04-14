extern crate regex;
use regex::Regex;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::fs::File;
use std::io;
use std::io::{BufRead, BufReader, Lines, Read, Write};
use std::path::Path;
use std::vec::Vec;

const TXN_SEP_REGEX: &str = r"\*{55}";
const TRACE_REGEX: &str = r" TRACE     : (\d{6})";
const SUCCESS_CWD_REGEX: &str = r"( RESP CODE : 00 \r*\n TRN TYPE  : CASH WITHDRAWAL)|( RESP CODE : 00 \r*\n TRN TYPE  : FAST CASH)";

struct JournalFile {
    terminal_id: String,
    date_time: String,
    path: String,
}

impl Clone for JournalFile {
    fn clone(&self) -> JournalFile {
        JournalFile {
            terminal_id: self.terminal_id.clone(),
            date_time: self.date_time.clone(),
            path: self.path.clone(),
        }
    }
}

struct JournalIterator {
    journals: Vec<JournalFile>,
    iterator: Option<Lines<BufReader<File>>>,
    current: usize,
}

impl JournalIterator {
    pub fn new(files: Vec<JournalFile>) -> JournalIterator {
        JournalIterator {
            journals: files,
            iterator: None,
            current: 0,
        }
    }
}

impl Iterator for JournalIterator {
    type Item = String;
    fn next(&mut self) -> Option<String> {
        let current = self.current;
        let path = self.journals[current].path.clone();
        let iterator = self.iterator.get_or_insert_with(|| {
            let file_d = File::open(path).unwrap();
            BufReader::new(file_d).lines()
        });
        match iterator.next() {
            Some(line) => Some(line.unwrap()),
            None => {
                if self.current + 1 < self.journals.len() {
                    self.current = self.current + 1;
                    self.iterator = None;
                    self.next()
                } else {
                    None
                }
            }
        }
    }
}

struct Transaction {
    text: String,
    complete: bool,
    trace: Vec<String>,
    successful_cwd: bool,
}

struct TransactionIterator {
    journal_iterator: JournalIterator,
    trace_regex: Regex,
    txn_sep_regex: Regex,
    txn_success_regex: Regex,
}

impl TransactionIterator {
    pub fn new(ji: JournalIterator) -> TransactionIterator {
        TransactionIterator {
            journal_iterator: ji,
            trace_regex: Regex::new(TRACE_REGEX).unwrap(),
            txn_sep_regex: Regex::new(TXN_SEP_REGEX).unwrap(),
            txn_success_regex: Regex::new(SUCCESS_CWD_REGEX).unwrap(),
        }
    }
}

impl Iterator for TransactionIterator {
    type Item = Transaction;
    fn next(&mut self) -> Option<Transaction> {
        let mut lines = Vec::new();
        let mut sep_occurences = 0;
        loop {
            let next = self.journal_iterator.next();
            if next.is_none() {
                return None;
            }
            let line = next.unwrap();
            if self.txn_sep_regex.is_match(&line) {
                sep_occurences = sep_occurences + 1;
                if lines.len() > 0 {
                    break; // Transaction end
                } else {
                    continue; // Transaction start
                }
            }
            lines.push(line);
        }
        if lines.len() == 0 {
            return None;
        }
        let text = lines.join("\n");
        let successful_cwd = self.txn_success_regex.is_match(&text);
        let traces: Vec<String> = self
            .trace_regex
            .captures_iter(&text)
            .map(|c| String::from(c.get(1).unwrap().as_str()))
            .collect();
        Some(Transaction {
            text: lines.join("\n"),
            complete: sep_occurences == 2,
            trace: traces,
            successful_cwd: successful_cwd,
        })
    }
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

fn get_files(path: String) -> Vec<JournalFile> {
    let paths = fs::read_dir(path).unwrap();
    let re = Regex::new(r"(\d{8})-(\d{4}-\d{1,2}-\d{1,2}).txt$").unwrap();
    let mut vec = Vec::new();
    for path in paths {
        let dir_entry = path.unwrap();
        let file_name = dir_entry.file_name().into_string().unwrap();
        match re.captures(&file_name) {
            Some(cap) => vec.push(JournalFile {
                terminal_id: String::from(&cap[1]),
                date_time: String::from(&cap[2]),
                path: dir_entry.path().into_os_string().into_string().unwrap(),
            }),
            None => {}
        }
    }
    vec
}

fn group_by_tid(files: Vec<JournalFile>) -> HashMap<String, Vec<JournalFile>> {
    let mut grouped_files: HashMap<String, Vec<JournalFile>> = HashMap::new();
    for file in files {
        grouped_files
            .entry(file.terminal_id.clone())
            .or_insert(Vec::new())
            .push(file);
    }
    grouped_files
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

fn main() {
    let trace = read_trace();
    let path = read_path(
        "Enter directory name containing journal logs (leave empty for current folder): ",
    );
    let files = get_files(String::from(path));
    let mut grouped = group_by_tid(files);

    // iterate EJs files line by line for each terminal
    for (terminal_id, files) in grouped.iter_mut() {
        files.sort_by(|a, b| a.date_time.cmp(&b.date_time));
        let journal_iterator = JournalIterator::new(files.to_vec());
        let txn_iterator = TransactionIterator::new(journal_iterator);
        let mut txns: Vec<Transaction> = Vec::new();
        let mut found = false;
        let mut before_counter = 0;
        let mut after_counter = 0;

        for txn in txn_iterator {
            if !found && before_counter > 3 {
                while before_counter > 3 {
                    if txns[0].successful_cwd {
                        before_counter -= 1;
                    }
                    txns.remove(0);
                }
            }

            found = found || txn.trace.contains(&String::from(trace.clone()));

            if found && after_counter >= 3 {
                break;
            }
            if found && txn.successful_cwd {
                after_counter += 1;
            }
            if !found && txn.successful_cwd {
                before_counter += 1;
            }
            txns.push(txn);
        }

        if !found {
            println!("Transaction with trace {} not found!", trace);
        } else {
            println!(
                "Transaction with trace #{} found for TID {}",
                trace, terminal_id
            );
            let lines: Vec<String> = txns.iter().map(|t| t.text.clone()).collect();
            let mut output = String::new();
            output.push_str("*******************************************************\n");
            output.push_str(
                &lines.join("\n*******************************************************\n"),
            );
            output.push_str("*******************************************************");
            save_file(&output);
        }
    }

    io::stdin().read(&mut [0]).unwrap();
}

use std::path::Path;

pub enum Status {
    Identified { source: String, dest: String },
    Unmatched { source: String },
    Error { source: String, message: String },
}

pub fn report(status: &Status) {
    match status {
        Status::Identified { source, dest } => {
            println!("[OK]        {}", Path::new(source).file_name().unwrap_or_default().to_string_lossy());
            println!("            -> {}", dest);
        }
        Status::Unmatched { source } => {
            println!("[UNMATCHED] {}", source);
        }
        Status::Error { source, message } => {
            println!("[ERROR]     {}: {}", source, message);
        }
    }
}

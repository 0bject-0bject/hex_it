//
// Copyright 2023, [object Object]
// Licensed under MIT
//

use std::{
    env,
    fs::File,
    io::{self, Read, Write},
    path::PathBuf,
    process,
    sync::mpsc,
    thread,
};
use tui_tools::{cls, getch, same_line_input, Colors};

fn print_help(args: &[String], commands: &[(&str, char, &str)]) {
    // clear the screen
    cls();

    println!("Usage: {} [OPTIONS]\n", args[0]);
    println!("Commands:");

    for command in commands {
        println!("-{}, --{} - {}", command.1, command.0, command.2);
    }
}

fn get_file_arg(args: &[String], i: usize) -> PathBuf {
    // Skip the file name!!
    PathBuf::from(args[i + 2].clone())
}

fn print_unknown_command_error(arg: &String, commands: &[(&str, char, &str)]) {
    let mut closest_match = (0, String::new());

    for command in commands {
        let distance = levenshtein_distance(arg, command.0);

        if distance < closest_match.0 || closest_match.0 == 0 {
            closest_match = (distance, command.0.to_string());
        }
    }

    eprintln!(
        "Unknown command '{}'. Did you mean '{}'?",
        arg, closest_match.1
    );
}

struct CliArgs {
    colors: bool,
    file: PathBuf,
}

const COMMANDS: [(&str, char, &str); 3] = [
    ("help", 'h', "Prints the help menu"),
    ("colors", 'c', "Enables ansi colors"),
    ("path", 'p', "The file to edit"),
];

fn get_args() -> CliArgs {
    let args: Vec<String> = env::args().collect();

    let mut cliargs = CliArgs {
        colors: false,
        file: PathBuf::new(),
    };

    let mut skip_next = false;
    for (i, arg) in args.iter().skip(1).enumerate() {
        if skip_next {
            skip_next = false;
            continue;
        }

        let mut found_command = false;
        for command in &COMMANDS {
            if arg == &format!("-{}", command.1) || arg == &format!("--{}", command.0) {
                found_command = true;
                match (command.0, command.1) {
                    ("help", 'h') => {
                        print_help(&args, &COMMANDS);
                        std::process::exit(0);
                    }
                    ("colors", 'c') => {
                        cliargs.colors = true;
                    }
                    ("path", 'p') => {
                        skip_next = true;
                        cliargs.file = get_file_arg(&args, i);
                    }
                    _ => {
                        unreachable!("Unknown command");
                    }
                }
            }
        }

        if !found_command {
            print_unknown_command_error(arg, &COMMANDS);
            std::process::exit(0);
        }
    }

    if cliargs.file.to_str().unwrap().is_empty() {
        eprintln!("No file specified.");
        std::process::exit(1);
    }

    cliargs
}

fn hex_to_bytes(hex_string: &str) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut byte = 0;
    let mut nibble_count = 0;

    // Iterate over each char
    for c in hex_string.chars() {
        // Convert each char to a 4-bit integer (a nibble)
        let nibble = match c.to_digit(16) {
            // If the character is a valid, convert it to a u8
            Some(nibble) => nibble as u8,
            None => return None,
        };

        // Shift the current byte left by 4 bits and OR it with the nibble
        byte = (byte << 4) | nibble;

        // Increment the nibble count
        nibble_count += 1;

        // If we've processed two nibbles, push the byte to the output vector
        // and reset the byte and nibble count
        if nibble_count == 2 {
            bytes.push(byte);
            byte = 0;
            nibble_count = 0;
        }
    }

    // If there are any remaining nibbles, the input is invalid, return None
    if nibble_count != 0 {
        None
    } else {
        // Otherwise, return the output vector of bytes
        Some(bytes)
    }
}

fn levenshtein_distance(string1: &str, string2: &str) -> usize {
    let len1 = string1.chars().count();
    let len2 = string2.chars().count();
    let mut matrix = vec![vec![0; len2 + 1]; len1 + 1];

    for i in 0..=len1 {
        matrix[i][0] = i;
    }

    for j in 0..=len2 {
        matrix[0][j] = j;
    }

    for i in 1..=len1 {
        for j in 1..=len2 {
            let cost = if string1.chars().nth(i - 1) == string2.chars().nth(j - 1) {
                0
            } else {
                1
            };
            matrix[i][j] = (matrix[i - 1][j] + 1) // deletion
                .min(matrix[i][j - 1] + 1) // insertion
                .min(matrix[i - 1][j - 1] + cost); // substitution

            if i > 1
                && j > 1
                && string1.chars().nth(i - 1) == string2.chars().nth(j - 2)
                && string1.chars().nth(i - 2) == string2.chars().nth(j - 1)
            {
                matrix[i][j] = matrix[i][j].min(matrix[i - 2][j - 2] + cost); // transposition
            }
        }
    }

    matrix[len1][len2]
}

fn move_cursor_bottom(command_string: &str) {
    let terminal_dimensions = term_size::dimensions().unwrap();
    let mut stdout = io::stdout();

    write!(
        stdout,
        "\x1b[{};{}H\x1b[2K{}",
        terminal_dimensions.1, 0, command_string
    )
    .unwrap();
}

#[derive(Clone)]
struct EditorState {
    file: PathBuf,
    offset: i32,
    colors: bool,
    hex_lines: Vec<String>,
    pretty_print: String,
}

impl EditorState {
    fn new(file: PathBuf, offset: i32, hex_lines: Vec<String>, colors: bool) -> EditorState {
        EditorState {
            file,
            offset,
            colors,
            hex_lines,
            pretty_print: String::new(),
        }
    }

    /// Parses the file into chunks of 16 bytes and returns them as a vector of strings
    fn parse_file(&mut self) -> Vec<String> {
        // Open the file
        let mut file = File::open(self.file.clone()).expect("Failed to open file");

        let mut contents = String::new();
        file.read_to_string(&mut contents)
            .expect("Failed to read file contents");

        // Split the contents into chunks of 16 bytes
        for chunk in contents.as_bytes().chunks(16) {
            let mut hex_chunk = String::new();

            // Split each chunk into groups of 2 bytes and convert them to hex
            for group in chunk.chunks(2) {
                for byte in group {
                    // Add the byte to the hex chunk with a space for pretty printing
                    hex_chunk.push_str(&format!("{:02X} ", byte));
                }
            }
            self.hex_lines.push(hex_chunk);
        }

        // Return the vector of hex lines ex: ["00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 48", "65 6C 6C 6F 20 77 6F 72 6C 64 21 20 20 20 20 20"]
        self.hex_lines.clone()
    }

    /// Generates the pretty printed message and returns it as a string
    fn generate_message(&mut self) -> String {
        let mut message = String::new();

        // Reset the offset to 0
        self.offset = 0;

        for line in &self.hex_lines {
            // Remove the spaces from the hex line
            let pure_hex = line.replace(' ', "");

            // Convert the hex line into bytes
            let bytes = hex_to_bytes(pure_hex.as_str()).expect("Failed to decode hex string");

            // Enable ansi colors based on the flag
            let newline_replacement = if self.colors {
                ".".bold_black()
            } else {
                ".".to_string()
            };

            let text = String::from_utf8(bytes)
                .expect("Failed to convert bytes to string")
                .replace('\n', newline_replacement.as_str());

            message += &format!("{:08X}  {:<48}  {}\n", self.offset, line, text);

            // Increment the offset by 16 bytes for the next line
            self.offset += 16;
        }

        message
    }

    /// Prints the pretty printed message to the console
    fn print(&mut self) {
        // Generate the pretty printed message if it hasn't been generated yet
        if self.pretty_print.is_empty() {
            self.pretty_print = self.generate_message();
        }

        print!("{}", self.pretty_print);

        // Print the command line area
        println!("{}", "-".repeat(80));
    }
}

struct CommandLine {
    editor: EditorState,
    command_names: Vec<(String, String)>,
}

impl CommandLine {
    fn new(editor: EditorState) -> CommandLine {
        let commands = vec![
            ("help", "Prints the help menu"),
            ("quit", "Quit the menu"),
            ("get", "Get a line of hex and be able to edit it."),
            ("save", "Save the file"),
        ];

        CommandLine {
            editor,
            command_names: commands
                .iter()
                .map(|(name, description)| (name.to_string(), description.to_string()))
                .collect(),
        }
    }

    fn new_command(&mut self, command: String) {
        let args = command
            .split(' ')
            .map(|s| s.to_string())
            .collect::<Vec<String>>();

        self.parse_command(args);
    }

    fn parse_command(&mut self, args: Vec<String>) {
        match args[0].to_ascii_lowercase().as_str() {
            "help" => {
                println!("Commands:");

                for command in &self.command_names {
                    println!("{} - {}", command.0, command.1);
                }

                same_line_input("Press enter to continue: ");
            }
            "quit" => {
                std::process::exit(0);
            }
            "get" => {
                if let Some(line) = args.get(1) {
                    if line.is_empty() {
                        return eprintln!("No line specified.");
                    }

                    // Convert the hex string to a decimal value
                    let decimal_value = i32::from_str_radix(line, 16).unwrap_or_else(|e| {
                        eprintln!("Invalid value '{line}': {e}");
                        std::process::exit(1);
                    }) / 16;

                    // Check if the line is out of range
                    if decimal_value as usize > self.editor.hex_lines.len() {
                        return eprintln!("Line out of range.");
                    }

                    let line_found = self.editor.hex_lines[decimal_value as usize].clone();

                    println!("{}", line_found);

                    println!("{}", "-".repeat(80));

                    let input = same_line_input("");

                    if input.is_empty() {
                        return;
                    }

                    // Replace the line with the new input
                    self.editor.hex_lines[decimal_value as usize] = input;

                    // Regenerate the pretty print
                    self.editor.pretty_print = self.editor.generate_message();
                } else {
                    eprintln!("No line specified.")
                }
            }
            "save" => {
                let file_path = args
                    .get(1)
                    .unwrap_or(&self.editor.file.to_str().unwrap().to_string())
                    .clone();

                let mut file = File::create(&file_path).unwrap();

                let contents = self
                    .editor
                    .hex_lines
                    .iter()
                    .map(|line| {
                        let pure_hex = line.replace(' ', "");
                        // Converts the hex line into bytes
                        let bytes = hex_to_bytes(pure_hex.as_str()).unwrap();

                        String::from_utf8(bytes).unwrap()
                    })
                    .collect::<String>();

                file.write_all(contents.as_bytes()).unwrap();

                println!(
                    "Saved to {}",
                    PathBuf::from(file_path)
                        .canonicalize()
                        .unwrap()
                        .to_str()
                        .unwrap()
                );

                std::process::exit(0);
            }
            _ if args[0].is_empty() => {}
            _ => {
                let mut closest_match = (0, String::new());
                for command in &self.command_names {
                    let distance = levenshtein_distance(&args[0], &command.0);

                    if distance < closest_match.0 || closest_match.0 == 0 {
                        closest_match = (distance, command.0.clone());
                    }
                }

                println!(
                    "Unknown command '{}'. Did you mean '{}'?",
                    args[0], closest_match.1
                );

                same_line_input("type 'help' for a list of commands. press enter to continue:");
            }
        }
    }
}

fn main() {
    // Get command line arguments
    let args = get_args();

    // Create a channel for sending keypresses from the main thread to the getch thread
    let (tx, rx) = mpsc::channel();
    let getch_thread = thread::spawn(move || loop {
        let key = getch();
        if tx.send(key).is_err() || key == 27 || key == 3 {
            break;
        }
        thread::sleep(std::time::Duration::from_millis(10));
    });

    // Initialize editor state
    let mut editor = EditorState::new(args.file, 0, Vec::new(), args.colors);
    editor.hex_lines = editor.parse_file();
    editor.print();

    // Initialize command line state
    let mut command_string = String::new();
    let mut command_line = CommandLine::new(editor.clone());
    let mut stdout = io::stdout();

    // Main loop
    loop {
        if let Ok(key) = rx.try_recv() {
            match key {
                27 => {
                    break;
                }
                3 => {
                    process::exit(0);
                }
                13 => {
                    editor.print();
                    command_line.new_command(command_string.clone());
                    editor = command_line.editor.clone();
                    editor.print();
                    command_string.clear();
                }
                _ => {
                    if (key > 64 && key < 90)
                        || (key > 96 && key < 123)
                        || (key > 47 && key < 58)
                        || key == 32
                    {
                        command_string.push(key as char);
                    }
                    if key == 8 {
                        command_string.pop();
                    }
                    // move the cursor to the bottom of the screen
                    move_cursor_bottom(&command_string);
                    stdout.flush().unwrap();
                }
            }
        }
    }

    getch_thread.join().unwrap();
}

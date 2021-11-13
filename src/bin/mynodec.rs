#[macro_use]
extern crate clap;
extern crate mynode;
extern crate rustyline;

use rustyline::error::ReadlineError;

fn main() -> Result<(), mynode::Error> {
    let opts = app_from_crate!()
        .arg(clap::Arg::with_name("command").short("c"))
        .arg(
            clap::Arg::with_name("headers")
                .short("H")
                .long("headers")
                .help("Show column headers"),
        )
        .arg(
            clap::Arg::with_name("host")
                .short("h")
                .long("host")
                .help("Host to connect to")
                .takes_value(true)
                .required(true)
                .default_value("127.0.0.1"),
        )
        .arg(
            clap::Arg::with_name("port")
                .short("p")
                .long("port")
                .help("Port number to connect to")
                .takes_value(true)
                .required(true)
                .default_value("9605"),
        )
        .get_matches();

    let mut mynode = MyNodeConsole::new(
        opts.value_of("host").unwrap(),
        opts.value_of("port").unwrap().parse()?,
    )?;
    if opts.is_present("headers") {
        mynode.show_headers = true
    }

    if let Some(command) = opts.value_of("command") {
        mynode.execute(&command)
    } else {
        mynode.run()
    }
}

/// MyNode REPL
struct MyNodeConsole {
    client: mynode::Client,
    editor: rustyline::Editor<()>,
    history_path: Option<std::path::PathBuf>,
    show_headers: bool,
}

impl MyNodeConsole {
    /// Creates a new ToySQL REPL for the given server host and port
    fn new(host: &str, port: u16) -> Result<Self, mynode::Error> {
        Ok(Self {
            client: mynode::Client::new(host, port)?,
            editor: rustyline::Editor::<()>::new(),
            history_path: std::env::var_os("HOME")
                .map(|home| std::path::Path::new(&home).join(".toysql.history")),
            show_headers: false,
        })
    }

    /// Executes a line of input
    fn execute(&mut self, input: &str) -> Result<(), mynode::Error> {
        if input.starts_with('!') {
            self.execute_command(&input)
        } else if !input.is_empty() {
            self.execute_query(&input)
        } else {
            Ok(())
        }
    }

    /// Runs the ToySQL REPL
    fn run(&mut self) -> Result<(), mynode::Error> {
        if let Some(path) = &self.history_path {
            match self.editor.load_history(path) {
                Ok(_) => {}
                Err(ReadlineError::Io(ref err)) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => return Err(err.into()),
            };
        }

        let status = self.client.status()?;
        println!(
            "Connected to node \"{}\" (version {}). Enter !help for instructions.",
            status.id, status.version
        );

        while let Some(input) = self.prompt()? {
            if let Err(err) = self.execute(&input) {
                println!("Error: {}", err.to_string())
            }
        }

        if let Some(path) = &self.history_path {
            self.editor.save_history(path)?;
        }
        Ok(())
    }

    /// Runs a query and displays the results
    fn execute_query(&mut self, query: &str) -> Result<(), mynode::Error> {
        let resultset = self.client.query(query)?;
        if self.show_headers {
            println!("{}", resultset.columns().join("|"));
        }
        for result in resultset {
            let formatted: Vec<String> = result?.into_iter().map(|v| format!("{}", v)).collect();
            println!("{}", formatted.join("|"));
        }
        Ok(())
    }

    /// Handles a REPL command (prefixed by !, e.g. !help)
    fn execute_command(&mut self, input: &str) -> Result<(), mynode::Error> {
        let mut input = input.split_ascii_whitespace();
        let command = input
            .next()
            .ok_or_else(|| mynode::Error::Parse("Expected command.".to_string()))?;

        let getargs = |n| {
            let args: Vec<&str> = input.collect();
            if args.len() != n {
                Err(mynode::Error::Parse(format!(
                    "{}: expected {} args, got {}",
                    command,
                    n,
                    args.len()
                )))
            } else {
                Ok(args)
            }
        };

        match command {
            "!headers" => match getargs(1)?[0] {
                "on" => {
                    self.show_headers = true;
                    println!("Headers enabled");
                }
                "off" => {
                    self.show_headers = false;
                    println!("Headers disabled");
                }
                v => {
                    return Err(mynode::Error::Parse(format!(
                        "Invalid value {}, expected on or off",
                        v
                    )))
                }
            },
            "!help" => println!(
                r#"
Enter an SQL statement on a single line to execute it and display the result.
Semicolons are not supported. The following !-commands are also available:
    !headers <on|off>  Toggles/enables/disables column headers display
    !help              This help message
    !table [table]     Display table schema, if it exists
"#
            ),
            "!table" => {
                let args = getargs(1)?;
                println!("{}", self.client.get_table(args[0])?);
            }
            c => return Err(mynode::Error::Parse(format!("Unknown command {}", c))),
        }
        Ok(())
    }

    /// Prompts the user for input
    fn prompt(&mut self) -> Result<Option<String>, mynode::Error> {
        match self.editor.readline("mynode> ") {
            Ok(input) => {
                self.editor.add_history_entry(&input);
                Ok(Some(input.trim().to_string()))
            }
            Err(ReadlineError::Eof) | Err(ReadlineError::Interrupted) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }
}

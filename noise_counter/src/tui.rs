use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, Borders, Cell, List, ListItem, ListState, Paragraph, Row, Table, TableState,
};
use ratatui::{DefaultTerminal, Frame};

use noise_core::count::{
    compute_run_totals, count_file, count_sequence_across, examine_file, parse_length_filter,
    parse_target_sequence, start_counting_session, CountingSession, CountResult, ExamineEntry,
    ExamineResult, RunTotals, SampleHit, SequenceQueryResult,
};
use noise_core::scan::{list_directory_for_browse, results_dir, DirectoryListing};

const APP_NAME: &str = "Noise Counter";
const APP_VERSION: &str = "v0.6.0";

const PROGRAM_FUNCTIONS: &[&str] = &[
    "• Counts duplicate DNA sequences in plain-text *_unbinned_reads files",
    "• One sequence per line; trailing N bases are trimmed before counting",
    "• Browse folders: Enter opens a folder or uses a single file",
    "• Space counts every *_unbinned_reads file in the current folder",
    "• Examine the first file's top 10 reads, then choose a length filter",
    "• s counts how many reads contain a sequence you type",
    "• Zero-hit samples show their closest read (mismatch diagnostic)",
    "• Writes one CSV per sample: sequence, counts, length",
];

enum BrowseEntry {
    Parent(PathBuf),
    Folder(PathBuf),
    ReadFile(PathBuf),
}

enum Screen {
    Welcome,
    FolderBrowse {
        current: PathBuf,
        entries: Vec<BrowseEntry>,
        read_files: Vec<PathBuf>,
        list_state: ListState,
    },
    Examine {
        input_folder: PathBuf,
        files: Vec<PathBuf>,
        results_dir: PathBuf,
        examine: ExamineResult,
        length_filter: String,
        filter_cursor: usize,
    },
    FileResult {
        session: CountingSession,
        result: CountResult,
    },
    AllComplete {
        session: CountingSession,
        totals: RunTotals,
    },
    SequenceQuery {
        folder: PathBuf,
        read_files: Vec<PathBuf>,
        single: Option<PathBuf>,
        all: bool,
        sequence: String,
        cursor: usize,
    },
    SequenceResult {
        folder: PathBuf,
        result: SequenceQueryResult,
        table_state: TableState,
    },
}

pub struct App {
    screen: Screen,
    status: String,
    status_is_error: bool,
    should_quit: bool,
}

enum PendingAction {
    NavigateFolder(PathBuf),
    UseSingleFile(PathBuf),
    UseFolder {
        folder: PathBuf,
        files: Vec<PathBuf>,
    },
    StartCounting {
        input_folder: PathBuf,
        files: Vec<PathBuf>,
        filter: Option<usize>,
    },
    OpenFolderBrowser(PathBuf),
    GoToWelcome,
    OpenSequenceQuery {
        folder: PathBuf,
        read_files: Vec<PathBuf>,
        single: Option<PathBuf>,
    },
    RunSequenceQuery {
        folder: PathBuf,
        files: Vec<PathBuf>,
        sequence: String,
    },
    ProcessNextFile(CountingSession),
    ProcessAllRemaining(CountingSession),
}

impl App {
    fn new() -> Self {
        Self {
            screen: Screen::Welcome,
            status: "Ready".to_string(),
            status_is_error: false,
            should_quit: false,
        }
    }

    fn set_status(&mut self, message: impl Into<String>) {
        self.status = message.into();
        self.status_is_error = false;
    }

    fn set_error(&mut self, message: String) {
        self.status = message;
        self.status_is_error = true;
    }

    fn clear_status(&mut self) {
        self.status = "Ready".to_string();
        self.status_is_error = false;
    }

    fn open_folder_browser(&mut self, folder: PathBuf) {
        match list_directory_for_browse(&folder) {
            Ok(listing) => {
                let n = listing.read_files.len();
                self.screen = folder_browser_screen(listing);
                if n == 0 {
                    self.set_status("No *_unbinned_reads files here — open a folder or go up");
                } else if n == 1 {
                    self.set_status("1 *_unbinned_reads file here — Space to count this folder");
                } else {
                    self.set_status(format!(
                        "{n} *_unbinned_reads files here — Space to count this folder"
                    ));
                }
            }
            Err(err) => self.set_error(err),
        }
    }

    fn examine_inputs(&mut self, input_folder: PathBuf, files: Vec<PathBuf>) {
        let Some(first) = files.first().cloned() else {
            self.set_error("No *_unbinned_reads files to process".to_string());
            return;
        };

        let label = first
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        self.set_status(format!("Examining {label}..."));

        match examine_file(&first) {
            Ok(examine) => {
                let results_dir = results_dir(&input_folder);
                self.screen = Screen::Examine {
                    input_folder,
                    files,
                    results_dir,
                    examine,
                    length_filter: String::new(),
                    filter_cursor: 0,
                };
                self.set_status(
                    "Top 10 shown — enter a length filter (or leave blank) and press Enter",
                );
            }
            Err(err) => self.set_error(err),
        }
    }

    fn open_sequence_query(
        &mut self,
        folder: PathBuf,
        read_files: Vec<PathBuf>,
        single: Option<PathBuf>,
    ) {
        if read_files.is_empty() {
            self.set_error("No *_unbinned_reads files in this folder".to_string());
            return;
        }
        // Default to the highlighted file when one is selected, otherwise all files.
        let all = single.is_none();
        self.screen = Screen::SequenceQuery {
            folder,
            read_files,
            single,
            all,
            sequence: String::new(),
            cursor: 0,
        };
        self.set_status("Type a sequence (A/C/G/T), choose the target, then press Enter");
    }

    fn run_sequence_query(&mut self, folder: PathBuf, files: Vec<PathBuf>, sequence: String) {
        self.set_status(format!("Counting reads containing {sequence}..."));
        let results_dir = results_dir(&folder);
        match count_sequence_across(&files, &sequence, &results_dir) {
            Ok(result) => {
                self.status = format!(
                    "Done — {} read(s) contain the sequence across {} file(s)",
                    result.total,
                    result.hits.len()
                );
                self.status_is_error = false;
                let mut table_state = TableState::default();
                if !result.hits.is_empty() {
                    table_state.select(Some(0));
                }
                self.screen = Screen::SequenceResult {
                    folder,
                    result,
                    table_state,
                };
            }
            Err(err) => self.set_error(err),
        }
    }

    fn start_counting(&mut self, input_folder: PathBuf, files: Vec<PathBuf>, filter: Option<usize>) {        self.set_status("Starting sequence count...");
        match start_counting_session(&input_folder, files, filter) {
            Ok(session) => self.count_current_file(session),
            Err(err) => self.set_error(err),
        }
    }

    fn count_current_file(&mut self, session: CountingSession) {
        if let Some(path) = session.current_file() {
            let label = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            self.set_status(format!("Counting {label}..."));
        }

        match self.run_count(&session) {
            Ok(result) => {
                let mut session = session;
                session.record_result(result.clone());
                self.screen = Screen::FileResult { session, result };
                self.set_status("Count complete — n/Enter: next file, a: all remaining");
            }
            Err(err) => self.set_error(err),
        }
    }

    fn run_count(&self, session: &CountingSession) -> Result<CountResult, String> {
        let path = session
            .current_file()
            .ok_or_else(|| "No *_unbinned_reads files to process".to_string())?;

        count_file(path, &session.results_dir, session.length_filter)
    }

    fn finish_counting(&mut self, session: CountingSession) {
        let totals = compute_run_totals(&session.completed);
        self.screen = Screen::AllComplete { session, totals };
        self.set_status("All files processed — review totals below");
    }

    fn proceed_to_next_file(&mut self, mut session: CountingSession) {
        if session.has_next() {
            session.advance();
            self.count_current_file(session);
        } else {
            self.finish_counting(session);
        }
    }

    fn process_all_remaining(&mut self, mut session: CountingSession) {
        while session.has_next() {
            session.advance();
            let label = session
                .current_file()
                .and_then(|p| p.file_name())
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            self.set_status(format!(
                "Counting {label}... ({}/{})",
                session.current_index + 1,
                session.read_files.len()
            ));

            match self.run_count(&session) {
                Ok(result) => session.record_result(result),
                Err(err) => {
                    self.set_error(err);
                    return;
                }
            }
        }

        self.finish_counting(session);
    }

    fn handle_key(&mut self, key: event::KeyEvent) {
        if key.code == KeyCode::Char('q') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.should_quit = true;
            return;
        }

        let mut action = None;

        match &mut self.screen {
            Screen::Welcome => match key.code {
                KeyCode::Esc | KeyCode::Char('q') => self.should_quit = true,
                KeyCode::Enter => {
                    let start = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                    action = Some(PendingAction::OpenFolderBrowser(start));
                }
                _ => {}
            },
            Screen::FolderBrowse {
                current,
                entries,
                read_files,
                list_state,
            } => match key.code {
                KeyCode::Esc => action = Some(PendingAction::GoToWelcome),
                KeyCode::Char('q') => self.should_quit = true,
                KeyCode::Up | KeyCode::Char('k') => {
                    let i = list_state.selected().unwrap_or(0);
                    list_state.select(Some(i.saturating_sub(1)));
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let i = list_state.selected().unwrap_or(0);
                    let next = (i + 1).min(entries.len().saturating_sub(1));
                    list_state.select(Some(next));
                }
                KeyCode::Enter => {
                    if let Some(idx) = list_state.selected() {
                        match entries.get(idx) {
                            Some(BrowseEntry::Parent(path) | BrowseEntry::Folder(path)) => {
                                action = Some(PendingAction::NavigateFolder(path.clone()));
                            }
                            Some(BrowseEntry::ReadFile(path)) => {
                                action = Some(PendingAction::UseSingleFile(path.clone()));
                            }
                            None => {}
                        }
                    }
                }
                KeyCode::Char(' ') => {
                    if read_files.is_empty() {
                        self.set_error("No *_unbinned_reads files in this folder".to_string());
                    } else {
                        action = Some(PendingAction::UseFolder {
                            folder: current.clone(),
                            files: read_files.clone(),
                        });
                    }
                }
                KeyCode::Char('s') => {
                    let single = match list_state.selected().and_then(|i| entries.get(i)) {
                        Some(BrowseEntry::ReadFile(path)) => Some(path.clone()),
                        _ => None,
                    };
                    action = Some(PendingAction::OpenSequenceQuery {
                        folder: current.clone(),
                        read_files: read_files.clone(),
                        single,
                    });
                }
                _ => {}
            },
            Screen::Examine {
                input_folder,
                files,
                length_filter,
                filter_cursor,
                ..
            } => match key.code {
                KeyCode::Esc => {
                    action = Some(PendingAction::OpenFolderBrowser(input_folder.clone()));
                }
                KeyCode::Char('q') => self.should_quit = true,
                KeyCode::Char(c) if c.is_ascii_digit() => {
                    length_filter.insert(*filter_cursor, c);
                    *filter_cursor += 1;
                    self.clear_status();
                }
                KeyCode::Backspace => {
                    if *filter_cursor > 0 {
                        *filter_cursor -= 1;
                        length_filter.remove(*filter_cursor);
                        self.clear_status();
                    }
                }
                KeyCode::Delete => {
                    if *filter_cursor < length_filter.len() {
                        length_filter.remove(*filter_cursor);
                        self.clear_status();
                    }
                }
                KeyCode::Left => {
                    if *filter_cursor > 0 {
                        *filter_cursor -= 1;
                    }
                }
                KeyCode::Right => {
                    if *filter_cursor < length_filter.len() {
                        *filter_cursor += 1;
                    }
                }
                KeyCode::Home => *filter_cursor = 0,
                KeyCode::End => *filter_cursor = length_filter.len(),
                KeyCode::Up => adjust_filter(length_filter, filter_cursor, 10),
                KeyCode::Down => adjust_filter(length_filter, filter_cursor, -10),
                KeyCode::Enter => match parse_length_filter(length_filter) {
                    Ok(filter) => {
                        action = Some(PendingAction::StartCounting {
                            input_folder: input_folder.clone(),
                            files: files.clone(),
                            filter,
                        });
                    }
                    Err(err) => self.set_error(err),
                },
                _ => {}
            },
            Screen::FileResult { .. } => match key.code {
                KeyCode::Esc | KeyCode::Char('q') => self.should_quit = true,
                KeyCode::Enter | KeyCode::Char('n') => {
                    if let Screen::FileResult { session, .. } = &self.screen {
                        action = Some(PendingAction::ProcessNextFile(session.clone()));
                    }
                }
                KeyCode::Char('a') => {
                    if let Screen::FileResult { session, .. } = &self.screen {
                        if session.has_next() {
                            action = Some(PendingAction::ProcessAllRemaining(session.clone()));
                        }
                    }
                }
                _ => {}
            },
            Screen::AllComplete { session, .. } => match key.code {
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter => self.should_quit = true,
                KeyCode::Char('r') => {
                    action = Some(PendingAction::OpenFolderBrowser(session.input_folder.clone()));
                }
                _ => {}
            },
            Screen::SequenceQuery {
                folder,
                read_files,
                single,
                all,
                sequence,
                cursor,
            } => match key.code {
                KeyCode::Esc => action = Some(PendingAction::OpenFolderBrowser(folder.clone())),
                KeyCode::Char('q') => self.should_quit = true,
                KeyCode::Tab => {
                    if single.is_some() {
                        *all = !*all;
                    }
                }
                KeyCode::Char(c) if c.is_ascii_alphabetic() => {
                    let upper = c.to_ascii_uppercase();
                    if matches!(upper, 'A' | 'C' | 'G' | 'T') {
                        sequence.insert(*cursor, upper);
                        *cursor += 1;
                        self.clear_status();
                    }
                }
                KeyCode::Backspace => {
                    if *cursor > 0 {
                        *cursor -= 1;
                        sequence.remove(*cursor);
                        self.clear_status();
                    }
                }
                KeyCode::Delete => {
                    if *cursor < sequence.len() {
                        sequence.remove(*cursor);
                        self.clear_status();
                    }
                }
                KeyCode::Left => {
                    if *cursor > 0 {
                        *cursor -= 1;
                    }
                }
                KeyCode::Right => {
                    if *cursor < sequence.len() {
                        *cursor += 1;
                    }
                }
                KeyCode::Home => *cursor = 0,
                KeyCode::End => *cursor = sequence.len(),
                KeyCode::Enter => {
                    let files = if *all {
                        read_files.clone()
                    } else if let Some(path) = single {
                        vec![path.clone()]
                    } else {
                        read_files.clone()
                    };
                    action = Some(PendingAction::RunSequenceQuery {
                        folder: folder.clone(),
                        files,
                        sequence: sequence.clone(),
                    });
                }
                _ => {}
            },
            Screen::SequenceResult {
                folder,
                result,
                table_state,
            } => match key.code {
                KeyCode::Char('q') | KeyCode::Enter => self.should_quit = true,
                KeyCode::Esc => action = Some(PendingAction::OpenFolderBrowser(folder.clone())),
                KeyCode::Up | KeyCode::Char('k') => {
                    let i = table_state.selected().unwrap_or(0);
                    table_state.select(Some(i.saturating_sub(1)));
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let i = table_state.selected().unwrap_or(0);
                    let last = result.hits.len().saturating_sub(1);
                    table_state.select(Some((i + 1).min(last)));
                }
                KeyCode::PageUp => {
                    let i = table_state.selected().unwrap_or(0);
                    table_state.select(Some(i.saturating_sub(10)));
                }
                KeyCode::PageDown => {
                    let i = table_state.selected().unwrap_or(0);
                    let last = result.hits.len().saturating_sub(1);
                    table_state.select(Some((i + 10).min(last)));
                }
                KeyCode::Home => {
                    if !result.hits.is_empty() {
                        table_state.select(Some(0));
                    }
                }
                KeyCode::End => {
                    if !result.hits.is_empty() {
                        table_state.select(Some(result.hits.len() - 1));
                    }
                }
                _ => {}
            },
        }

        match action {
            Some(PendingAction::NavigateFolder(folder)) => self.open_folder_browser(folder),
            Some(PendingAction::UseSingleFile(path)) => {
                let folder = path
                    .parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| PathBuf::from("."));
                self.examine_inputs(folder, vec![path]);
            }
            Some(PendingAction::UseFolder { folder, files }) => {
                self.examine_inputs(folder, files);
            }
            Some(PendingAction::StartCounting {
                input_folder,
                files,
                filter,
            }) => self.start_counting(input_folder, files, filter),
            Some(PendingAction::OpenFolderBrowser(folder)) => self.open_folder_browser(folder),
            Some(PendingAction::OpenSequenceQuery {
                folder,
                read_files,
                single,
            }) => self.open_sequence_query(folder, read_files, single),
            Some(PendingAction::RunSequenceQuery {
                folder,
                files,
                sequence,
            }) => match parse_target_sequence(&sequence) {
                Ok(sequence) => self.run_sequence_query(folder, files, sequence),
                Err(err) => self.set_error(err),
            },
            Some(PendingAction::GoToWelcome) => {
                self.screen = Screen::Welcome;
                self.clear_status();
            }
            Some(PendingAction::ProcessNextFile(session)) => self.proceed_to_next_file(session),
            Some(PendingAction::ProcessAllRemaining(session)) => {
                self.process_all_remaining(session);
            }
            None => {}
        }
    }
}

fn folder_browser_screen(listing: DirectoryListing) -> Screen {
    let mut entries = Vec::new();
    if let Some(parent) = listing.parent {
        entries.push(BrowseEntry::Parent(parent));
    }
    for folder in listing.subfolders {
        entries.push(BrowseEntry::Folder(folder));
    }
    for file in &listing.read_files {
        entries.push(BrowseEntry::ReadFile(file.clone()));
    }

    let mut list_state = ListState::default();
    if !entries.is_empty() {
        list_state.select(Some(0));
    }

    Screen::FolderBrowse {
        current: listing.current,
        entries,
        read_files: listing.read_files,
        list_state,
    }
}

fn program_info_lines() -> Vec<Line<'static>> {
    PROGRAM_FUNCTIONS
        .iter()
        .map(|line| Line::from(*line))
        .collect()
}

pub fn run() -> io::Result<()> {
    let mut terminal = ratatui::init();
    let result = run_app(&mut terminal);
    ratatui::restore();
    result
}

fn run_app(terminal: &mut DefaultTerminal) -> io::Result<()> {
    let mut app = App::new();

    loop {
        terminal
            .draw(|frame| draw(frame, &mut app))
            .map_err(|err| io::Error::other(err.to_string()))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    app.handle_key(key);
                }
            }
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(12),
            Constraint::Length(3),
        ])
        .split(area);

    draw_header(frame, chunks[0]);

    match &mut app.screen {
        Screen::Welcome => draw_welcome(frame, chunks[1]),
        Screen::FolderBrowse {
            current,
            entries,
            list_state,
            ..
        } => draw_folder_browse(frame, chunks[1], current, entries, list_state),
        Screen::Examine {
            input_folder,
            files,
            results_dir,
            examine,
            length_filter,
            filter_cursor,
            ..
        } => draw_examine(
            frame,
            chunks[1],
            input_folder,
            files,
            results_dir,
            examine,
            length_filter,
            *filter_cursor,
        ),
        Screen::FileResult { session, result } => draw_file_result(frame, chunks[1], session, result),
        Screen::AllComplete { session, totals } => {
            draw_all_complete(frame, chunks[1], session, totals);
        }
        Screen::SequenceQuery {
            folder,
            read_files,
            single,
            all,
            sequence,
            cursor,
        } => draw_sequence_query(
            frame,
            chunks[1],
            folder,
            read_files,
            single.as_ref(),
            *all,
            sequence,
            *cursor,
        ),
        Screen::SequenceResult {
            folder,
            result,
            table_state,
        } => {
            draw_sequence_result(frame, chunks[1], folder, result, table_state);
        }
    }

    draw_status(frame, chunks[2], app);
}

fn draw_header(frame: &mut Frame, area: Rect) {
    let title = format!("🧬  {APP_NAME}  {APP_VERSION}");
    let header = Paragraph::new(Line::from(Span::styled(
        title,
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )))
    .alignment(Alignment::Center)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    frame.render_widget(header, area);
}

fn panel_block(title: &str) -> Block<'static> {
    Block::default()
        .title(format!(" {title} "))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
}

fn centered(line: Line<'static>, style: Style) -> Paragraph<'static> {
    Paragraph::new(line).alignment(Alignment::Center).style(style)
}

fn enter_key_span() -> Span<'static> {
    Span::styled(" Enter ", Style::default().fg(Color::Black).bg(Color::Green))
}

fn quit_key_span() -> Span<'static> {
    Span::styled(" q ", Style::default().fg(Color::Black).bg(Color::Red))
}

fn draw_welcome(frame: &mut Frame, area: Rect) {
    let block = panel_block("Welcome");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(20),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(4),
            Constraint::Percentage(20),
            Constraint::Length(1),
            Constraint::Percentage(20),
        ])
        .split(inner);

    frame.render_widget(
        centered(
            Line::from(format!("Welcome to \"{APP_NAME}\"")),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        chunks[1],
    );
    frame.render_widget(Paragraph::new("Program functions:"), chunks[2]);

    let functions = Paragraph::new(Text::from(program_info_lines()));
    frame.render_widget(functions, chunks[3]);

    frame.render_widget(
        centered(
            Line::from(vec![
                Span::styled(" Enter ", Style::default().fg(Color::Black).bg(Color::Green)),
                Span::raw("   Browse folders     "),
                Span::styled(" q ", Style::default().fg(Color::Black).bg(Color::Red)),
                Span::raw("   Quit"),
            ]),
            Style::default(),
        ),
        chunks[5],
    );
}

fn draw_folder_browse(
    frame: &mut Frame,
    area: Rect,
    current: &Path,
    entries: &[BrowseEntry],
    list_state: &mut ListState,
) {
    let block = panel_block("Browse *_unbinned_reads Files");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(5),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "Current folder: ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(current.display().to_string()),
        ])),
        chunks[0],
    );
    frame.render_widget(
        Paragraph::new("Select a folder to open or a *_unbinned_reads file to use:"),
        chunks[1],
    );

    let items: Vec<ListItem> = entries
        .iter()
        .map(|entry| match entry {
            BrowseEntry::Parent(_) => ListItem::new("../"),
            BrowseEntry::Folder(path) => {
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                ListItem::new(format!("{name}/"))
            }
            BrowseEntry::ReadFile(path) => {
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                ListItem::new(Line::from(Span::styled(
                    name,
                    Style::default().fg(Color::Cyan),
                )))
            }
        })
        .collect();

    let list = List::new(items)
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");
    frame.render_stateful_widget(list, chunks[2], list_state);

    frame.render_widget(
        centered(
            Line::from(vec![
                Span::styled("↑/↓", Style::default().fg(Color::Green)),
                Span::raw("   Select     "),
                enter_key_span(),
                Span::raw("   Open / use file     "),
                Span::styled(
                    " Space ",
                    Style::default().fg(Color::Black).bg(Color::Green),
                ),
                Span::raw("   Count folder     "),
                Span::styled(" s ", Style::default().fg(Color::Black).bg(Color::Cyan)),
                Span::raw("   Count sequence     "),
                Span::styled("Esc", Style::default().fg(Color::Green)),
                Span::raw("   Back     "),
                quit_key_span(),
                Span::raw("   Quit"),
            ]),
            Style::default(),
        ),
        chunks[4],
    );
}

fn draw_examine(
    frame: &mut Frame,
    area: Rect,
    input_folder: &Path,
    files: &[PathBuf],
    results_dir: &Path,
    examine: &ExamineResult,
    length_filter: &str,
    filter_cursor: usize,
) {
    let block = panel_block("Examine First File");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // title
            Constraint::Length(1), // files + output context
            Constraint::Length(1), // stats
            Constraint::Min(12),   // top-10 table
            Constraint::Length(3), // length filter prompt + field
            Constraint::Length(1), // footer keys
        ])
        .split(inner);

    let title = format!(
        "Examining first file: {} ({} of {} selected)",
        examine.sample_name,
        1,
        files.len()
    );
    frame.render_widget(
        centered(
            Line::from(title),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        chunks[0],
    );

    frame.render_widget(
        Paragraph::new(format!(
            "{} file(s) selected · input: {} · output: {}",
            files.len(),
            input_folder.display(),
            results_dir.display()
        )),
        chunks[1],
    );

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw("Lines: "),
            Span::styled(
                examine.lines_read.to_string(),
                Style::default().fg(Color::White),
            ),
            Span::raw("   Counted: "),
            Span::styled(
                examine.sequences_used.to_string(),
                Style::default().fg(Color::Cyan),
            ),
            Span::raw("   Skipped: "),
            Span::styled(
                examine.lines_skipped.to_string(),
                Style::default().fg(Color::Yellow),
            ),
            Span::raw("   Unique: "),
            Span::styled(
                examine.unique_sequences.to_string(),
                Style::default().fg(Color::Green),
            ),
        ])),
        chunks[2],
    );

    let seq_width = sequence_width(inner.width);
    let table = Paragraph::new(Text::from(top10_lines(&examine.top, seq_width)));
    frame.render_widget(table, chunks[3]);

    let prompt_and_field = Paragraph::new(Text::from(vec![
        Line::from(vec![
            Span::styled(
                "Length filter",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(
                " (bases 1..N) — type digits or ↑/↓ (±10); blank = full length; Enter to apply.",
            ),
        ]),
        input_field_line(length_filter, filter_cursor, "e.g. 30"),
    ]));
    frame.render_widget(prompt_and_field, chunks[4]);

    frame.render_widget(
        centered(
            Line::from(vec![
                Span::styled("digits", Style::default().fg(Color::Green)),
                Span::raw(" / "),
                Span::styled("↑↓ ±10", Style::default().fg(Color::Green)),
                Span::raw("   Set length     "),
                enter_key_span(),
                Span::raw("   Apply and count     "),
                Span::styled("Esc", Style::default().fg(Color::Green)),
                Span::raw("   Back     "),
                quit_key_span(),
                Span::raw("   Quit"),
            ]),
            Style::default(),
        ),
        chunks[5],
    );
}

fn draw_sequence_query(
    frame: &mut Frame,
    area: Rect,
    folder: &Path,
    read_files: &[PathBuf],
    single: Option<&PathBuf>,
    all: bool,
    sequence: &str,
    cursor: usize,
) {
    let block = panel_block("Count a Specific Sequence");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // title
            Constraint::Length(1), // folder
            Constraint::Length(1), // target
            Constraint::Length(1), // prompt
            Constraint::Length(1), // input
            Constraint::Min(1),    // spacer
            Constraint::Length(1), // footer
        ])
        .split(inner);

    frame.render_widget(
        centered(
            Line::from("Count how many reads contain a sequence"),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        chunks[0],
    );
    frame.render_widget(
        Paragraph::new(format!("Folder: {}", folder.display())),
        chunks[1],
    );

    let use_all = all || single.is_none();
    let target = if use_all {
        format!("Target: all {} file(s) in this folder", read_files.len())
    } else {
        let name = single
            .and_then(|p| p.file_name())
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        format!("Target: {name}")
    };
    let target_line = if single.is_some() {
        format!("{target}    (Tab switches file/all)")
    } else {
        target
    };
    frame.render_widget(Paragraph::new(target_line), chunks[2]);

    frame.render_widget(Paragraph::new("Sequence (A/C/G/T):"), chunks[3]);
    frame.render_widget(
        Paragraph::new(input_field_line(sequence, cursor, "e.g. ACGTACGT")),
        chunks[4],
    );

    let mut keys: Vec<Span> = vec![
        enter_key_span(),
        Span::raw(if use_all { " Count all" } else { " Count file" }),
    ];
    if single.is_some() {
        keys.push(Span::raw("   "));
        keys.push(Span::styled("Tab", Style::default().fg(Color::Green)));
        keys.push(Span::raw(" Toggle target"));
    }
    keys.push(Span::raw("   "));
    keys.push(Span::styled("Esc", Style::default().fg(Color::Green)));
    keys.push(Span::raw(" Back   "));
    keys.push(quit_key_span());
    keys.push(Span::raw(" Quit"));
    frame.render_widget(centered(Line::from(keys), Style::default()), chunks[6]);
}

fn draw_sequence_result(
    frame: &mut Frame,
    area: Rect,
    folder: &Path,
    result: &SequenceQueryResult,
    table_state: &mut TableState,
) {
    let block = panel_block("Sequence Count");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // title
            Constraint::Length(1), // folder
            Constraint::Length(1), // explanation
            Constraint::Min(3),    // per-sample table
            Constraint::Length(1), // total
            Constraint::Length(1), // output path
            Constraint::Length(1), // footer
        ])
        .split(inner);

    frame.render_widget(
        centered(
            Line::from(format!("Reads containing {}", result.sequence)),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        chunks[0],
    );
    frame.render_widget(
        Paragraph::new(format!("Folder: {}", folder.display())),
        chunks[1],
    );
    frame.render_widget(
        Paragraph::new(
            "One count per read. For samples with 0, the nearest read window is shown; mismatched bases are red.",
        ),
        chunks[2],
    );

    let rows: Vec<Row> = result
        .hits
        .iter()
        .map(|hit| {
            Row::new(vec![
                Cell::from(hit.sample_name.clone()),
                Cell::from(hit.count.to_string()),
                closest_match_cell(hit, &result.sequence),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(16),
            Constraint::Percentage(9),
            Constraint::Percentage(75),
        ],
    )
    .header(
        Row::new(vec!["Sample", "Reads", "Closest match when 0"]).style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(
        Block::default()
            .title(" Per sample — ↑/↓ or PgUp/PgDn to scroll ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray)),
    )
    .column_spacing(2)
    .row_highlight_style(
        Style::default()
            .bg(Color::DarkGray)
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )
    .highlight_symbol("▸ ");
    frame.render_stateful_widget(table, chunks[3], table_state);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Total: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(
                result.total.to_string(),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
        ])),
        chunks[4],
    );
    frame.render_widget(
        Paragraph::new(format!("Output: {}", result.output_path.display())),
        chunks[5],
    );
    frame.render_widget(
        centered(
            Line::from(vec![
                Span::styled("↑/↓", Style::default().fg(Color::Green)),
                Span::raw(" Scroll   "),
                Span::styled("q", Style::default().fg(Color::Green)),
                Span::raw(" / "),
                enter_key_span(),
                Span::raw(" Quit   "),
                Span::styled("Esc", Style::default().fg(Color::Green)),
                Span::raw(" Back"),
            ]),
            Style::default(),
        ),
        chunks[6],
    );
}

/// Builds the diagnostics cell for one sample. Non-zero samples show a dash;
/// zero-hit samples show the closest equal-length window, with mismatched
/// bases highlighted in red.
fn closest_match_cell(hit: &SampleHit, query: &str) -> Cell<'static> {
    if hit.count > 0 {
        return Cell::from("—");
    }

    let Some(closest) = &hit.closest else {
        return Cell::from(format!("(no read with ≥ {} bases)", query.len()));
    };

    let label = if closest.distance == 1 {
        "1 mismatch  ".to_string()
    } else {
        format!("{} mismatches  ", closest.distance)
    };

    let mut spans: Vec<Span<'static>> =
        vec![Span::styled(label, Style::default().fg(Color::Yellow))];
    for (read_base, query_base) in closest.window.chars().zip(query.chars()) {
        if read_base == query_base {
            spans.push(Span::raw(read_base.to_string()));
        } else {
            spans.push(Span::styled(
                read_base.to_string(),
                Style::default()
                    .fg(Color::Red)
                    .add_modifier(Modifier::BOLD),
            ));
        }
    }

    Cell::from(Line::from(spans))
}

/// Renders the editable length-filter field with a visible block cursor.
/// Uses the terminal's default foreground so the text stays readable on both
/// dark and light themes (the old black-on-black styling made it invisible).
fn input_field_line(value: &str, cursor: usize, hint: &str) -> Line<'static> {
    let value_style = Style::default().add_modifier(Modifier::BOLD);
    let cursor_style = Style::default()
        .fg(Color::Black)
        .bg(Color::Yellow)
        .add_modifier(Modifier::BOLD);

    let chars: Vec<char> = value.chars().collect();
    let cursor = cursor.min(chars.len());

    let mut spans: Vec<Span<'static>> = vec![Span::raw("  > ")];
    if cursor > 0 {
        spans.push(Span::styled(
            chars[..cursor].iter().collect::<String>(),
            value_style,
        ));
    }
    if cursor < chars.len() {
        // Block cursor sitting on the character under the caret.
        spans.push(Span::styled(chars[cursor].to_string(), cursor_style));
        if cursor + 1 < chars.len() {
            spans.push(Span::styled(
                chars[cursor + 1..].iter().collect::<String>(),
                value_style,
            ));
        }
    } else {
        // Block cursor at the end of the field.
        spans.push(Span::styled(" ", cursor_style));
        if chars.is_empty() {
            spans.push(Span::raw("   "));
            spans.push(Span::styled(
                hint.to_string(),
                Style::default().fg(Color::DarkGray),
            ));
        }
    }

    Line::from(spans)
}

/// Steps the numeric filter up/down by `delta`, keeping it non-negative.
/// An empty or unparsable value is treated as 0; zero is shown as blank.
fn adjust_filter(value: &mut String, cursor: &mut usize, delta: isize) {
    let current: isize = value.trim().parse().unwrap_or(0);
    let next = (current + delta).max(0);
    *value = if next == 0 {
        String::new()
    } else {
        next.to_string()
    };
    *cursor = value.chars().count();
}

fn top10_lines(entries: &[ExamineEntry], seq_width: usize) -> Vec<Line<'static>> {    const MARGIN: &str = "   ";
    const GAP_SEQ: &str = "  ";
    const GAP_COUNT: &str = "  ";
    const GAP_LEN: &str = "   ";

    let mut lines: Vec<Line<'static>> = Vec::new();

    let header = format!(
        "{MARGIN}{}{GAP_SEQ}{}{GAP_COUNT}{}{GAP_LEN}{}",
        rj("#", 2),
        lj("Sequence", seq_width),
        rj("Count", 7),
        rj("Length", 6),
    );
    lines.push(Line::from(Span::styled(
        header,
        Style::default().add_modifier(Modifier::BOLD),
    )));

    // Size ruler aligned directly under the sequence column.
    let ruler_col = MARGIN.len() + 2 + GAP_SEQ.len();
    let ruler = format!("{}{}", " ".repeat(ruler_col), make_ruler(seq_width));
    lines.push(Line::from(Span::styled(ruler, Style::default().fg(Color::DarkGray))));

    for (i, e) in entries.iter().enumerate() {
        let row = format!(
            "{MARGIN}{}{GAP_SEQ}{}{GAP_COUNT}{}{GAP_LEN}{}",
            rj(i + 1, 2),
            lj(&clipped_sequence(&e.sequence, seq_width), seq_width),
            rj(e.count, 7),
            rj(e.length, 6),
        );
        lines.push(Line::from(row));
    }

    if entries.is_empty() {
        lines.push(Line::from("(no sequences found)"));
    }

    lines
}

/// Right-justify a value into `width` columns.
fn rj(n: impl std::fmt::Display, width: usize) -> String {
    format!("{:>w$}", n, w = width)
}

/// Left-justify a string into `width` columns.
fn lj(s: &str, width: usize) -> String {
    format!("{:<w$}", s, w = width)
}

/// Builds a "size ruler" with labels at 1, 10, 20, ... right-aligned so each
/// label's last digit sits on that base position.
fn make_ruler(len: usize) -> String {
    let mut grid: Vec<char> = vec![' '; len];

    let mut positions: Vec<usize> = vec![1];
    let mut pos = 10usize;
    while pos <= len {
        positions.push(pos);
        pos += 10;
    }

    for pos in positions {
        let label = pos.to_string();
        let start = pos.saturating_sub(label.len());
        if start + label.len() <= len {
            for (i, ch) in label.chars().enumerate() {
                grid[start + i] = ch;
            }
        }
    }

    grid.into_iter().collect()
}

/// Clips a sequence to `width` characters, appending '…' when truncated. The
/// result is always exactly `width` characters wide.
fn clipped_sequence(seq: &str, width: usize) -> String {
    let chars: Vec<char> = seq.chars().collect();
    if chars.len() <= width {
        return seq.to_string();
    }
    let keep = width.saturating_sub(1);
    let mut out: String = chars[..keep].iter().collect();
    out.push('…');
    out
}

/// How many sequence characters fit in the table given the available width.
fn sequence_width(area_width: u16) -> usize {
    // MARGIN(3) + rank(2) + GAP_SEQ(2) + GAP_COUNT(2) + count(7) + GAP_LEN(3) + length(6)
    const FIXED: usize = 3 + 2 + 2 + 2 + 7 + 3 + 6;
    let avail = (area_width as usize).saturating_sub(FIXED);
    avail.clamp(20, 100)
}

fn draw_status(frame: &mut Frame, area: Rect, app: &App) {
    let text_style = if app.status_is_error {
        Style::default().fg(Color::Red)
    } else {
        // Default foreground keeps the text readable on dark and light themes.
        Style::default()
    };

    let status = Paragraph::new(Line::from(Span::styled(
        app.status.as_str(),
        text_style,
    )))
    .block(
        Block::default()
            .title(" Status ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    frame.render_widget(status, area);
}

fn draw_file_result(
    frame: &mut Frame,
    area: Rect,
    session: &CountingSession,
    result: &CountResult,
) {
    let block = panel_block("Sequence Count Results");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(inner);

    frame.render_widget(
        centered(
            Line::from(format!(
                "Finished: {}",
                result
                    .read_path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
            )),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        chunks[0],
    );
    frame.render_widget(Paragraph::new(session.progress_label()), chunks[1]);
    frame.render_widget(
        Paragraph::new(format!(
            "Sample: {}   Lines read: {}",
            result.sample_name, result.lines_read
        )),
        chunks[2],
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw("Sequences counted: "),
            Span::styled(
                result.sequences_used.to_string(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("   Skipped: "),
            Span::styled(
                result.lines_skipped.to_string(),
                Style::default().fg(Color::Yellow),
            ),
            Span::raw("   Unique: "),
            Span::styled(
                result.unique_sequences.to_string(),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
        ])),
        chunks[3],
    );
    frame.render_widget(
        Paragraph::new(format!("Length filter: {}", session.filter_label())),
        chunks[4],
    );
    frame.render_widget(
        Paragraph::new(format!("Output: {}", result.output_path.display())),
        chunks[5],
    );

    let preview = Paragraph::new(Text::from(
        result
            .preview_lines
            .iter()
            .map(|line| Line::from(line.as_str()))
            .collect::<Vec<_>>(),
    ));
    frame.render_widget(preview, chunks[6]);

    let footer = if session.has_next() {
        let remaining = session
            .read_files
            .len()
            .saturating_sub(session.current_index + 1);
        Line::from(vec![
            Span::raw("Continue: "),
            enter_key_span(),
            Span::raw(" / "),
            Span::styled(" n ", Style::default().fg(Color::Black).bg(Color::Green)),
            Span::raw(" Next only ("),
            Span::styled(
                remaining.to_string(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" left)  "),
            Span::styled(" a ", Style::default().fg(Color::Black).bg(Color::Green)),
            Span::raw(" All remaining  "),
            quit_key_span(),
            Span::raw(" Quit"),
        ])
    } else {
        Line::from(vec![
            enter_key_span(),
            Span::raw(" Finish  "),
            quit_key_span(),
            Span::raw(" Quit"),
        ])
    };

    frame.render_widget(centered(footer, Style::default()), chunks[7]);
}

fn draw_all_complete(
    frame: &mut Frame,
    area: Rect,
    session: &CountingSession,
    totals: &RunTotals,
) {
    let block = panel_block("Run Summary");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(5),
            Constraint::Length(1),
        ])
        .split(inner);

    frame.render_widget(
        centered(
            Line::from("All files counted"),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        chunks[0],
    );
    frame.render_widget(
        Paragraph::new(format!(
            "  Files: {}   Input: {}   Output: {}",
            totals.file_count,
            session.input_folder.display(),
            session.results_dir.display()
        )),
        chunks[1],
    );
    frame.render_widget(
        Paragraph::new(format!("  Length filter: {}", session.filter_label())),
        chunks[2],
    );

    let table = Table::new(
        vec![
            Row::new(vec![
                Cell::from("Lines read"),
                Cell::from(format_number(totals.total_lines)),
            ]),
            Row::new(vec![
                Cell::from("Sequences counted"),
                Cell::from(format_number(totals.total_used)),
            ]),
            Row::new(vec![
                Cell::from("Lines skipped"),
                Cell::from(format_number(totals.total_skipped)),
            ]),
            Row::new(vec![
                Cell::from("Total unique sequences (all files)"),
                Cell::from(format_number(totals.total_unique)),
            ]),
        ],
        [Constraint::Percentage(65), Constraint::Percentage(35)],
    )
    .header(
        Row::new(vec!["Metric", "Count"]).style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(
        Block::default()
            .title(" Totals ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray)),
    )
    .column_spacing(2);
    frame.render_widget(table, chunks[3]);

    frame.render_widget(
        centered(
            Line::from(vec![
                enter_key_span(),
                Span::raw(" Quit  "),
                Span::styled("r", Style::default().fg(Color::Green)),
                Span::raw(" Restart  "),
                quit_key_span(),
                Span::raw(" Quit"),
            ]),
            Style::default(),
        ),
        chunks[4],
    );
}

fn format_number(value: u64) -> String {
    value
        .to_string()
        .as_bytes()
        .rchunks(3)
        .rev()
        .map(|chunk| std::str::from_utf8(chunk).unwrap_or("0"))
        .collect::<Vec<_>>()
        .join(",")
}

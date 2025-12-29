use anyhow::{bail, Context, Result};
use clap::Parser;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Wrap},
    Terminal,
};
use std::{
    collections::HashSet,
    fs,
    io::{self, Stdout},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use syntect::{
    easy::HighlightLines,
    highlighting::{Theme, ThemeSet},
    parsing::{SyntaxReference, SyntaxSet},
    util::LinesWithEndings,
};
use walkdir::WalkDir;

#[derive(Parser, Debug)]
#[command(name = "codescroller", about = "Auto-scroll code files with syntax highlighting.")]
struct Args {
    /// A file or directory to scroll through
    #[arg(value_name = "PATH")]
    path: PathBuf,

    /// Delay between scroll steps in milliseconds
    #[arg(long, default_value_t = 60)]
    speed_ms: u64,

    /// Number of terminal lines to advance per tick
    #[arg(long, default_value_t = 1)]
    step: usize,

    /// Loop forever (when reaching end of file list, start over)
    #[arg(long, default_value_t = true)]
    r#loop: bool,

    /// Optional comma-separated extensions (no dots). Example: rs,cs,go,cpp,h,py,js,ts
    #[arg(long)]
    exts: Option<String>,

    /// Maximum file size to load (in KB). Larger files are skipped.
    #[arg(long, default_value_t = 512)]
    max_kb: u64,

    /// Start at a random file (requires OS randomness? no; deterministic-ish fallback)
    #[arg(long, default_value_t = false)]
    random_start: bool,
}

struct App {
    files: Vec<PathBuf>,
    file_index: usize,

    // Current file loaded
    current_path: PathBuf,
    raw: String,
    highlighted_lines: Vec<Line<'static>>,
    syntax_name: String,

    scroll: usize,
    paused: bool,

    // Rendering
    status: String,

    // Highlighting
    ps: SyntaxSet,
    theme: Theme,
}

fn main() -> Result<()> {
    let args = Args::parse();

    setup_terminal()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend).context("create terminal")?;

    let result = run(&mut terminal, args);

    restore_terminal()?;
    terminal.show_cursor().ok();

    result
}

fn setup_terminal() -> Result<()> {
    enable_raw_mode().context("enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).context("enter alt screen")?;
    Ok(())
}

fn restore_terminal() -> Result<()> {
    disable_raw_mode().ok();
    let mut stdout = io::stdout();
    execute!(stdout, LeaveAlternateScreen).ok();
    Ok(())
}

fn run(terminal: &mut Terminal<CrosstermBackend<Stdout>>, args: Args) -> Result<()> {
    let exts = parse_exts(args.exts.as_deref());
    let files = collect_files(&args.path, &exts, args.max_kb)
        .with_context(|| format!("collect files from {}", args.path.display()))?;

    if files.is_empty() {
        bail!("No matching code files found under {}", args.path.display());
    }

    let ps = SyntaxSet::load_defaults_newlines();
    let theme = pick_theme();

    let mut app = App {
        files,
        file_index: 0,

        current_path: PathBuf::new(),
        raw: String::new(),
        highlighted_lines: Vec::new(),
        syntax_name: String::new(),

        scroll: 0,
        paused: false,
        status: String::new(),

        ps,
        theme,
    };

    if args.random_start {
        app.file_index = pseudo_random_index(app.files.len());
    }

    load_current(&mut app)?;

    let tick = Duration::from_millis(args.speed_ms.max(5));
    let mut last_tick = Instant::now();

    loop {
        terminal.draw(|f| ui(f, &app))?;

        // Input (non-blocking with timeout until next tick)
        let timeout = tick.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)? {
            if let Event::Key(k) = event::read()? {
                if k.kind == KeyEventKind::Press {
                    match k.code {
                        KeyCode::Char('q') => return Ok(()),
                        KeyCode::Char(' ') => app.paused = !app.paused,
                        KeyCode::Char('n') | KeyCode::Right => {
                            next_file(&mut app, args.r#loop)?;
                        }
                        KeyCode::Char('p') | KeyCode::Left => {
                            prev_file(&mut app, args.r#loop)?;
                        }
                        KeyCode::Char('r') => {
                            load_current(&mut app)?;
                        }
                        KeyCode::Home => app.scroll = 0,
                        KeyCode::End => app.scroll = app.highlighted_lines.len().saturating_sub(1),
                        _ => {}
                    }
                }
            }
        }

        if last_tick.elapsed() >= tick {
            last_tick = Instant::now();
            if !app.paused {
                app.scroll = app.scroll.saturating_add(args.step);

                // When file ends, move to next
                if app.scroll >= app.highlighted_lines.len().saturating_sub(1) {
                    next_file(&mut app, args.r#loop)?;
                }
            }
        }
    }
}

fn ui(f: &mut ratatui::Frame, app: &App) {
    let size = f.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(1)].as_ref())
        .split(size);

    let title = format!(
        "{}  ({}/{})  [{}]  {}",
        app.current_path.display(),
        app.file_index + 1,
        app.files.len(),
        app.syntax_name,
        if app.paused { "PAUSED" } else { "PLAY" }
    );

    let header = Paragraph::new(Line::from(vec![
        Span::styled("codescroller", Style::default().fg(Color::Green)),
        Span::raw(" — "),
        Span::raw(title),
    ]))
    .block(Block::default().borders(Borders::BOTTOM));

    f.render_widget(header, chunks[0]);

    let view_height = chunks[1].height as usize;

    let start = app.scroll.min(app.highlighted_lines.len().saturating_sub(1));
    let end = (start + view_height).min(app.highlighted_lines.len());

    let mut text = Text::default();
    for line in &app.highlighted_lines[start..end] {
        text.lines.push(line.clone());
    }

    // If file is short, pad to avoid jitter
    while text.lines.len() < view_height {
        text.lines.push(Line::from(Span::raw("")));
    }

    let footer_hint = if app.status.is_empty() {
        "q quit • space pause • n/p next/prev • r reload • ←/→ also work"
    } else {
        app.status.as_str()
    };

    let body = Paragraph::new(text)
        .block(
            Block::default()
                .borders(Borders::NONE)
                .title(Line::from(Span::styled(
                    footer_hint,
                    Style::default().fg(Color::DarkGray),
                ))),
        )
        .wrap(Wrap { trim: false });

    f.render_widget(body, chunks[1]);
}

fn collect_files(root: &Path, exts: &HashSet<String>, max_kb: u64) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();

    if root.is_file() {
        if is_allowed(root, exts, max_kb)? {
            out.push(root.to_path_buf());
        }
        return Ok(out);
    }

    for entry in WalkDir::new(root).follow_links(false) {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let p = entry.path();
        if is_allowed(p, exts, max_kb)? {
            out.push(p.to_path_buf());
        }
    }

    out.sort();
    Ok(out)
}

fn is_allowed(p: &Path, exts: &HashSet<String>, max_kb: u64) -> Result<bool> {
    let meta = match fs::metadata(p) {
        Ok(m) => m,
        Err(_) => return Ok(false),
    };
    if meta.len() > max_kb.saturating_mul(1024) {
        return Ok(false);
    }

    // Exclude obvious junk
    if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
        if name.starts_with('.') && name.len() > 1 {
            // allow dotfiles if extension matches; keep it simple: skip dotfiles
            return Ok(false);
        }
    }

    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
    let ext = ext.to_lowercase();
    Ok(exts.contains(&ext))
}

fn parse_exts(s: Option<&str>) -> HashSet<String> {
    let default = [
        "rs", "toml", "c", "h", "cpp", "hpp", "cc", "cs", "go", "py", "js", "ts", "jsx", "tsx",
        "java", "kt", "swift", "php", "rb", "lua", "sh", "ps1", "sql", "html", "css", "json",
        "yml", "yaml", "md",
    ];

    let mut set = HashSet::new();
    let list: Vec<&str> = if let Some(s) = s {
        s.split(',').map(|x| x.trim()).filter(|x| !x.is_empty()).collect()
    } else {
        default.to_vec()
    };

    for e in list {
        set.insert(e.trim_start_matches('.').to_lowercase());
    }
    set
}

fn load_current(app: &mut App) -> Result<()> {
    app.scroll = 0;
    app.status.clear();

    let path = app.files[app.file_index].clone();
    let raw = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            app.status = format!("Skipping unreadable file: {} ({})", path.display(), e);
            // Try next file
            next_file(app, true)?;
            return Ok(());
        }
    };

    let syntax = pick_syntax(&app.ps, &path, &raw);
    let syntax_name = syntax.name.clone();

    let highlighted = highlight_to_tui_lines(&app.ps, &app.theme, syntax, &raw);

    app.current_path = path;
    app.raw = raw;
    app.highlighted_lines = highlighted;
    app.syntax_name = syntax_name;

    Ok(())
}

fn pick_syntax<'a>(ps: &'a SyntaxSet, path: &Path, raw: &str) -> &'a SyntaxReference {
    // Try extension first, then fallback to content-based, then plain text
    ps.find_syntax_for_file(path)
        .ok()
        .flatten()
        .or_else(|| ps.find_syntax_by_first_line(raw))
        .unwrap_or_else(|| ps.find_syntax_plain_text())
}

fn pick_theme() -> Theme {
    // Built-in themes; choose a high-contrast dark theme by default
    // (ThemeSet::load_defaults() includes common ones like "base16-ocean.dark")
    let ts = ThemeSet::load_defaults();
    ts.themes
        .get("base16-ocean.dark")
        .cloned()
        .unwrap_or_else(|| ts.themes.values().next().cloned().unwrap())
}

fn highlight_to_tui_lines(
    ps: &SyntaxSet,
    theme: &Theme,
    syntax: &SyntaxReference,
    raw: &str,
) -> Vec<Line<'static>> {
    let mut h = HighlightLines::new(syntax, theme);

    let mut out = Vec::new();
    for line in LinesWithEndings::from(raw) {
        let regions = h.highlight_line(line, ps).unwrap_or_default();
        let mut spans: Vec<Span<'static>> = Vec::with_capacity(regions.len());

        for (style, text) in regions {
            let fg = Color::Rgb(style.foreground.r, style.foreground.g, style.foreground.b);
            spans.push(Span::styled(text.to_string(), Style::default().fg(fg)));
        }
        out.push(Line::from(spans));
    }

    if out.is_empty() {
        out.push(Line::from(Span::raw("")));
    }
    out
}

fn next_file(app: &mut App, looping: bool) -> Result<()> {
    if app.file_index + 1 < app.files.len() {
        app.file_index += 1;
        load_current(app)?;
        return Ok(());
    }
    if looping {
        app.file_index = 0;
        load_current(app)?;
        return Ok(());
    }
    Ok(())
}

fn prev_file(app: &mut App, looping: bool) -> Result<()> {
    if app.file_index > 0 {
        app.file_index -= 1;
        load_current(app)?;
        return Ok(());
    }
    if looping {
        app.file_index = app.files.len().saturating_sub(1);
        load_current(app)?;
        return Ok(());
    }
    Ok(())
}

// Cheap deterministic "random-ish" start index without extra deps
fn pseudo_random_index(len: usize) -> usize {
    use std::time::{SystemTime, UNIX_EPOCH};
    if len == 0 {
        return 0;
    }
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    (n as usize) % len
}

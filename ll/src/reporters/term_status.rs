use super::Level;
use crate::task_tree::{TaskInternal, TaskResult, TaskStatus, TaskTree, TASK_TREE};
use crate::uniq_id::UniqID;
use anyhow::{Context, Result};
use colored::Colorize;
use crossterm::{cursor, style, terminal};
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};

const NOSTATUS_TAG: &str = "nostatus";

/// When true, `StdioReporter` should push lines to `LOG_BUFFER` instead
/// of writing directly to stderr.  The render loop drains the buffer
/// and writes those lines itself, so nothing else touches stderr while
/// the status frame is on screen.
static TERM_STATUS_ACTIVE: AtomicBool = AtomicBool::new(false);

lazy_static::lazy_static! {
    pub static ref TERM_STATUS: TermStatus = TermStatus::new(TASK_TREE.clone());
    static ref LOG_BUFFER: Mutex<Vec<String>> = Mutex::new(Vec::new());
}

/// Returns true when the live status tree is being rendered.
/// Used by `StdioReporter` to decide whether to buffer output.
pub fn is_active() -> bool {
    TERM_STATUS_ACTIVE.load(Ordering::Relaxed)
}

/// Push a formatted log line into the buffer.  The render loop will
/// drain these and write them to stderr before the next frame.
pub fn buffer_line(line: String) {
    LOG_BUFFER.lock().unwrap().push(line);
}

/// Drain all buffered log lines.
fn drain_buffer() -> Vec<String> {
    std::mem::take(&mut *LOG_BUFFER.lock().unwrap())
}

pub fn show() {
    // Only enable it if it's a TTY terminal, otherwise output
    // can get super messy.
    if crossterm::tty::IsTty::is_tty(&std::io::stderr()) {
        TERM_STATUS.show();
    }
}

pub fn hide() {
    TERM_STATUS.hide();
}

#[derive(Clone)]
pub struct TermStatus(Arc<RwLock<TermStatusInternal>>);

impl TermStatus {
    fn new(task_tree: Arc<TaskTree>) -> Self {
        Self(Arc::new(RwLock::new(TermStatusInternal::new(task_tree))))
    }

    pub fn show(&self) {
        let mut lock = self.0.write().unwrap();
        if lock.enabled {
            return;
        } else {
            lock.enabled = true;
        }
        TERM_STATUS_ACTIVE.store(true, Ordering::Relaxed);
        drop(lock);

        let t = self.clone();
        std::thread::spawn(move || {
            loop {
                // Sleep OUTSIDE the stdio lock.  Log lines produced
                // during this window go into LOG_BUFFER (non-blocking
                // push to a Vec), so nothing writes to stderr directly.
                std::thread::sleep(std::time::Duration::from_millis(50));

                let stdout = std::io::stdout();
                let stderr = std::io::stderr();

                // Lock stdio briefly — just long enough to clear the
                // old frame, flush buffered log lines, and draw the
                // new frame.  Total hold time is the write itself
                // (microseconds), not the display duration.
                let stdout_lock = stdout.lock();
                let mut stderr_lock = stderr.lock();

                let mut internal = t.0.write().unwrap();
                if !internal.enabled {
                    break;
                }

                internal.render_frame(&mut stderr_lock).ok();

                drop(stdout_lock);
                drop(stderr_lock);
            }
        });
    }

    pub fn hide(&self) {
        let mut internal = self.0.write().unwrap();
        internal.enabled = false;

        let stderr = std::io::stderr();
        let mut stderr_lock = stderr.lock();

        // Set ACTIVE to false only after holding the stderr lock.
        // Otherwise a StdioReporter could see is_active()==false,
        // eprintln! a line while the frame is still on screen, and
        // then clear_frame() would wipe that line along with the frame.
        TERM_STATUS_ACTIVE.store(false, Ordering::Relaxed);

        internal.clear_frame(&mut stderr_lock).ok();

        // Flush any remaining buffered lines now that the frame is gone.
        for line in drain_buffer() {
            writeln!(stderr_lock, "{}", line).ok();
        }
    }
}

/*
 Vec of indentations. Bool represents whether a vertical line needs to be
 at every point of the indentation, e.g.

    [▶] Root Task
    │
    ├ [✓] Task 1
    │  ╰ [▶] Task 3        <-- vec[true, true] has line
    ╰ [✓] Task 1
       ╰ [⨯] Failed task   <-- vec[false, true] no line
*/
type Depth = Vec<bool>;

#[derive(Clone)]
pub struct TermStatusInternal {
    current_height: usize,
    task_tree: Arc<TaskTree>,
    pub max_log_level: Level,
    enabled: bool,
}

impl TermStatusInternal {
    fn new(task_tree: Arc<TaskTree>) -> Self {
        Self {
            current_height: 0,
            task_tree,
            max_log_level: Level::default(),
            enabled: false,
        }
    }

    /// One render tick:
    ///   1. Erase the previous status frame
    ///   2. Write buffered log lines (they scroll into history)
    ///   3. Draw the new status frame
    ///
    /// Everything is queued into a single `Vec<u8>` buffer and written
    /// with one `write_all` + `flush`.  Wrapped in synchronized-output
    /// markers so the terminal paints it atomically — no flicker.
    fn render_frame(&mut self, w: &mut impl Write) -> Result<()> {
        let rows = self.make_status_rows()?;
        let buffered_lines = drain_buffer();
        let new_height = rows.len();

        // Nothing to draw and nothing to clear — skip entirely.
        if new_height == 0 && self.current_height == 0 && buffered_lines.is_empty() {
            return Ok(());
        }

        // Previous frame on screen but nothing new to draw — just clear it
        // and flush any buffered lines.  Don't leave a stale frame lingering.
        if new_height == 0 && buffered_lines.is_empty() && self.current_height > 0 {
            self.clear_frame(w)?;
            return Ok(());
        }

        let mut buf = Vec::with_capacity(4096);

        crossterm::queue!(&mut buf, terminal::BeginSynchronizedUpdate)?;

        // 1. Erase the previous frame.
        if self.current_height > 0 {
            crossterm::queue!(
                &mut buf,
                cursor::MoveUp((self.current_height + 1) as u16),
                terminal::Clear(terminal::ClearType::FromCursorDown)
            )?;
        }

        // 2. Flush buffered log lines — they appear above the frame
        //    and scroll into normal terminal history.
        for line in &buffered_lines {
            crossterm::queue!(&mut buf, style::Print(line), style::Print("\n"))?;
        }

        // 3. Draw the new frame.
        if !rows.is_empty() {
            let frame = format!("\n{}\n", rows.join("\n"));
            crossterm::queue!(&mut buf, style::Print(frame))?;
        }

        crossterm::queue!(&mut buf, terminal::EndSynchronizedUpdate)?;

        w.write_all(&buf)?;
        w.flush()?;

        self.current_height = new_height;
        Ok(())
    }

    /// Erase the current frame from the screen (used when hiding).
    fn clear_frame(&mut self, w: &mut impl Write) -> Result<()> {
        if self.current_height > 0 {
            let mut buf = Vec::with_capacity(256);
            crossterm::queue!(
                &mut buf,
                cursor::MoveUp((self.current_height + 1) as u16),
                terminal::Clear(terminal::ClearType::FromCursorDown)
            )?;
            w.write_all(&buf)?;
            w.flush()?;
            self.current_height = 0;
        }
        Ok(())
    }

    fn make_status_rows(&self) -> Result<Vec<String>> {
        let tree = self.task_tree.tree_internal.read().unwrap();
        let child_to_parents = tree.child_to_parents();
        let parent_to_children = tree.parent_to_children();

        let mut stack: Vec<(UniqID, Depth)> = tree
            .root_tasks()
            .iter()
            .filter(|id| !child_to_parents.contains_key(id))
            .map(|id| (*id, vec![]))
            .collect();

        let mut rows = vec![];
        while let Some((id, depth)) = stack.pop() {
            let task = tree.get_task(id).context("must be present")?;

            let dontprint = !self.should_print(task);

            let children_iter = parent_to_children.get(&id).into_iter().flatten().peekable();
            let mut append_to_stack = vec![];

            let last_visible_child = children_iter
                .clone()
                .filter(|id| tree.get_task(**id).map_or(false, |t| self.should_print(t)))
                .last();

            // we still need to DFS the ones that we don't print to make sure
            // we're not skipping their children
            for subtask_id in children_iter {
                let mut new_depth = depth.clone();
                // If we're not printing it, we're not adding the indent either
                // so this tasks children will become children of the parent task
                if !dontprint {
                    new_depth.push(Some(subtask_id) != last_visible_child);
                }
                append_to_stack.push((*subtask_id, new_depth));
            }

            // Since we're popping, we'll be going through children in reverse order,
            // so we need to counter that.
            append_to_stack.reverse();
            stack.append(&mut append_to_stack);

            if !dontprint {
                rows.push(self.task_row(task, depth)?);
            }
        }

        let (_, term_height) = crossterm::terminal::size().unwrap_or((50, 50));
        let max_height = term_height as usize - 2;

        if rows.len() > max_height {
            let trimmed = rows.len() - max_height;
            rows = rows.into_iter().take(max_height).collect();
            rows.push(format!(".......{} more tasks.......", trimmed))
        }

        Ok(rows)
    }

    fn should_print(&self, task: &TaskInternal) -> bool {
        let level = super::utils::parse_level(task);
        !task.tags.contains(NOSTATUS_TAG) && (level <= self.max_log_level)
    }

    fn task_row(&self, task_internal: &TaskInternal, mut depth: Depth) -> Result<String> {
        /*

        [▶] Root Task
        │
        ├ [✓] Task 1
        │  ╰ [▶] Task 3
        ├ [✓] Task 1
        ╰ [⨯] Failed task
        */

        let indent = if let Some(last_indent) = depth.pop() {
            // Worst case utf8 symbol pre level is 4 bytes
            let mut indent = String::with_capacity(4 * depth.len());
            for has_vertical_line in depth.into_iter() {
                if has_vertical_line {
                    indent.push_str("│ ");
                } else {
                    indent.push_str("  ");
                }
            }

            if last_indent {
                indent.push_str("├ ");
            } else {
                indent.push_str("╰ ");
            }

            indent
        } else {
            String::new()
        };

        let status = match task_internal.status {
            TaskStatus::Running => " ▶ ".black().on_yellow(),
            TaskStatus::Finished(TaskResult::Success, _) => " ✓ ".black().on_green(),
            TaskStatus::Finished(TaskResult::Failure(_), _) => " x ".white().on_red(),
        };

        let progress = make_progress(task_internal);

        let duration = match task_internal.status {
            TaskStatus::Finished(_, finished_at) => {
                finished_at.duration_since(task_internal.started_at)
            }
            _ => task_internal.started_at.elapsed(),
        }?;

        let secs = duration.as_secs();
        let millis = (duration.as_millis() % 1000) / 100;
        let ts = format!(" [{}.{}s] ", secs, millis).dimmed();

        Ok(format!(
            "{}{}{}{}{}",
            indent, status, ts, progress, task_internal.name
        ))
    }
}

fn make_progress(task: &TaskInternal) -> String {
    const PROGRESS_BAR_LEN: i64 = 30;

    if let Some((done, total)) = &task.progress {
        if *total == 0 {
            // otherwise we'll divide by 0 and it'll panic
            return String::new();
        }
        let pct_done = (done * 100) / total;
        let done_blocks_len = std::cmp::min((PROGRESS_BAR_LEN * pct_done) / 100, PROGRESS_BAR_LEN);
        let todo_blocks_len = PROGRESS_BAR_LEN - done_blocks_len;
        let done_blocks = " ".repeat(done_blocks_len as usize).on_bright_green();
        let todo_blocks = ".".repeat(todo_blocks_len as usize).on_black();
        format!(" [{}{}] {}/{} ", done_blocks, todo_blocks, done, total)
    } else {
        String::new()
    }
}

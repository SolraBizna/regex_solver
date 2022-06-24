use std::{
    io::Write,
    time::Instant,
};

use anyhow::{anyhow, Context};
use fancy_regex::Regex;
use once_cell::sync::Lazy;
use regex_syntax::ast::Ast;
use serde::Deserialize;

mod allowed;
use allowed::*;

static BACKREFERENCE_STRIPPING_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"\\[0-9]"#).unwrap()
});

#[derive(Deserialize)]
struct PuzzleSpec {
    width: usize,
    height: usize,
    top_hints: Option<Vec<Option<String>>>,
    left_hints: Option<Vec<Option<String>>>,
    bottom_hints: Option<Vec<Option<String>>>,
    right_hints: Option<Vec<Option<String>>>,
}

struct Hints {
    top: Vec<Option<Regex>>,
    bottom: Vec<Option<Regex>>,
    left: Vec<Option<Regex>>,
    right: Vec<Option<Regex>>,
}

struct Board {
    width: usize,
    height: usize,
    allowed_chars: Vec<Vec<u8>>,
    undecided_cells: Vec<(usize, usize)>,
    // Row and column complexity for each cell
    tree_complexity: Vec<(f64, f64)>,
    // Cells (and column-nesses) we've already tried without making progress
    progress_free_cells: Vec<(bool, usize, usize)>,
}

struct Choice {
    complexity: f64,
    is_column: bool,
    index: usize,
    x: usize, y: usize,
}

impl Hints {
    fn collect_hints(name: &str, src: Option<&Vec<Option<String>>>, length: usize) -> anyhow::Result<Vec<Option<Regex>>> {
        match src {
            None => Ok(vec![None; length]),
            Some(src) => {
                let ret: anyhow::Result<Vec<Option<Regex>>>
                = src.iter().enumerate().map(|(index, code)| {
                    match code {
                        Some(code) => {
                            match Regex::new(&format!("^{}$", code)) {
                                Ok(x) => Ok(Some(x)),
                                x => Err(x.context(format!("Couldn't compile {} hint #{}", name, index+1)).unwrap_err()),
                            }
                        },
                        None => Ok(None)
                    }
                }).collect();
                match ret {
                    Ok(mut x) => {
                        x.resize(length, None);
                        Ok(x)
                    },
                    Err(x) => Err(x),
                }
            }
        }
    }
    pub fn from_spec(spec: &PuzzleSpec) -> anyhow::Result<Hints> {
        // Check to make sure at least one hint is present for every row and
        // column
        let top_len = spec.top_hints.as_ref().map(|x| x.len()).unwrap_or(0);
        let bottom_len = spec.bottom_hints.as_ref().map(|x| x.len()).unwrap_or(0);
        let left_len = spec.left_hints.as_ref().map(|x| x.len()).unwrap_or(0);
        let right_len = spec.right_hints.as_ref().map(|x| x.len()).unwrap_or(0);
        if top_len.max(bottom_len) != spec.width {
            return Err(anyhow!("Puzzle does not have a vertical hint for every column!"))
        }
        if left_len.max(right_len) != spec.height {
            return Err(anyhow!("Puzzle does not have a horizontal hint for every row!"))
        }
        let top = Hints::collect_hints("top", spec.top_hints.as_ref(), spec.width)?;
        let bottom = Hints::collect_hints("bottom", spec.bottom_hints.as_ref(), spec.width)?;
        let left = Hints::collect_hints("left", spec.left_hints.as_ref(), spec.height)?;
        let right = Hints::collect_hints("right", spec.right_hints.as_ref(), spec.height)?;
        Ok(Hints { top, bottom, left, right })
    }
}

impl Board {
    fn new(width: usize, height: usize, row_allowed_chars: Vec<Vec<u8>>, col_allowed_chars: Vec<Vec<u8>>) -> Board {
        let mut board = Board {
            width, height,
            allowed_chars: Vec::with_capacity(width * height),
            undecided_cells: Vec::with_capacity(width * height),
            tree_complexity: Vec::with_capacity(width * height),
            progress_free_cells: Vec::with_capacity(width * height * 2), // one each for row and column of cell
        };
        // Okay, now we need to calculate the minimum set for each *cell*.
        // (Print any already-decided characters in bold)
        print!("\x1B[1m");
        for y in 0 .. height {
            for x in 0 .. width {
                let allowed = allowed_char_intersection(&row_allowed_chars[y], &col_allowed_chars[x]);
                if(allowed.len() == 0) {
                    print!("\x1B[33;7m");
                    board.print_cell(x, y, '0');
                    print!("\x1B[0m");
                    let _ = std::io::stdout().flush();
                    println!("Cell {},{} had no possibilities!", x + 1, y + 1);
                    println!("Row: {:?}", row_allowed_chars[y].iter().map(|&x| x as char).collect::<String>());
                    println!("Col: {:?}", col_allowed_chars[x].iter().map(|&x| x as char).collect::<String>());
                    std::process::exit(1);
                }
                else if allowed.len() == 1 {
                    board.print_cell(x, y, allowed[0] as char);
                }
                else {
                    board.undecided_cells.push((x, y));
                }
                board.allowed_chars.push(allowed);
            }
        }
        // (Turn bold back off)
        print!("\x1B[0m");
        // Calculate the tree complexities for the whole board.
        // (The tree complexity of a cell is the number of all possible
        // combinations we would have to investigate to test particular values
        // of that cell.)
        for y in 0 .. height {
            for x in 0 .. width {
                if board.allowed_chars(x, y).len() == 1 {
                    // Wellp.
                    board.tree_complexity.push((1.0, 1.0));
                }
                else {
                    let mut row_complexity = board.allowed_chars(x, y).len() as f64;
                    for x in 0 .. width {
                        row_complexity *= board.allowed_chars(x, y).len() as f64;
                    }
                    // Or, the Rusty way!
                    let col_complexity = (0 .. height)
                    .map(|y| {
                        board.allowed_chars(x, y).len()
                    }).fold(board.allowed_chars(x, y).len() as f64, |a, len| a * len as f64);
                    board.tree_complexity.push((row_complexity, col_complexity));
                }
            }
        }
        board
    }
    fn recalculate_tree_complexity(&mut self, x: usize, y: usize) {
        let row_complexity = (0 .. self.width)
        .map(|x| {
            self.allowed_chars(x, y).len()
        }).fold(self.allowed_chars(x, y).len() as f64, |a, len| a * len as f64);
        let col_complexity = (0 .. self.height)
        .map(|y| {
            self.allowed_chars(x, y).len()
        }).fold(self.allowed_chars(x, y).len() as f64, |a, len| a * len as f64);
        self.tree_complexity[x + y * self.width] = (row_complexity, col_complexity);
    }
    fn allowed_chars(&self, x: usize, y: usize) -> &Vec<u8> {
        assert!(x < self.width && y < self.height);
        &self.allowed_chars[x + y * self.width]
    }
    fn tree_complexity(&self, x: usize, y: usize) -> (f64, f64) {
        assert!(x < self.width && y < self.height);
        self.tree_complexity[x + y * self.width]
    }
    fn still_undecided(&self) -> bool {
        self.undecided_cells.len() > 0
    }
    fn maybe_best_choice(&self, best_choice: &mut Option<Choice>, candidate: Choice) {
        // First, make sure this choice isn't blacklisted!
        if let Ok(_) = self.progress_free_cells.binary_search(&(candidate.is_column, candidate.x, candidate.y)) {
            // We found the candidate in the progress_free_cells list. Die.
            return
        }
        if let Some(best_choice) = best_choice {
            if best_choice.complexity < candidate.complexity {
                // The best choice is better than us. Die.
                return
            }
        }
        // We are the best choice now!
        *best_choice = Some(candidate);
    }
    // Returns true if any progress was made
    fn make_progress(&mut self, hints: &Hints) -> bool {
        // Find the LOWEST tree complexity in the unsolved portion.
        let mut best_choice: Option<Choice> = None;
        for (index, &(x, y)) in self.undecided_cells.iter().enumerate() {
            let (row_complexity, col_complexity) = self.tree_complexity(x, y);
            let row_choice = Choice {
                complexity: row_complexity, is_column: false, index, x, y
            };
            let col_choice = Choice {
                complexity: col_complexity, is_column: true, index, x, y
            };
            self.maybe_best_choice(&mut best_choice, row_choice);
            self.maybe_best_choice(&mut best_choice, col_choice);
        }
        if best_choice.is_none() { return false } // No progress is possible.
        let Choice { x, y, index, is_column, complexity } = best_choice.unwrap();
        self.print_cell(x, y, if is_column { '|' } else { '-' });
        // Brute force that cell! Find out all ACTUALLY possible characters!
        let possible = self.allowed_chars(x, y);
        let mut really_possible = Vec::with_capacity(possible.len());
        let possibilities: Vec<&Vec<u8>>;
        let open_cell: Vec<bool>;
        let mut big_number: Vec<usize>;
        let mut buf: Vec<u8>;
        if is_column {
            // it's a column
            possibilities = (0 .. self.height).map(|y| self.allowed_chars(x, y)).collect();
            open_cell = (0 .. self.height).map(|cell_y| cell_y != y && possibilities[cell_y].len() > 1).collect();
            big_number = vec![0; self.height];
            buf = vec![0; self.height];
            for &ch in possible.iter() {
                // Clear the big number and the buffer
                for y in 0 .. self.height {
                    big_number[y] = 0;
                    buf[y] = self.allowed_chars(x, y)[0];
                }
                // Put the character we're brute forcing in the right slot
                buf[y] = ch;
                // And... uh... start.
                let mut any_allowed = false;
                'trying_col: while !any_allowed {
                    // Try this string!
                    let as_str = unsafe { std::str::from_utf8_unchecked(&buf) };
                    let mut allowed = true;
                    if let Some(ref hint) = hints.top[x] {
                        if !hint.is_match(as_str).unwrap() {
                            allowed = false;
                        }
                    }
                    if allowed {
                        if let Some(ref hint) = hints.bottom[x] {
                            if !hint.is_match(as_str).unwrap() {
                                allowed = false;
                            }
                        }
                    }
                    if allowed {
                        any_allowed = true;
                        break;
                    }
                    else {
                        // Move on to the next possibility...
                        for i in 0 .. self.height {
                            if !open_cell[i] { continue }
                            let nu = big_number[i] + 1;
                            if nu >= possibilities[i].len() {
                                big_number[i] = 0;
                            }
                            else { big_number[i] = nu }
                            buf[i] = possibilities[i][big_number[i]];
                            if nu < possibilities[i].len() {
                                continue 'trying_col;
                            }
                        }
                        break 'trying_col; // we ran off the end of the big_number
                    }
                }
                if any_allowed {
                    really_possible.push(ch);
                }
            }
        }
        else {
            // it's a row
            possibilities = (0 .. self.width).map(|x| self.allowed_chars(x, y)).collect();
            open_cell = (0 .. self.width).map(|cell_x| cell_x != x && possibilities[cell_x].len() > 1).collect();
            big_number = vec![0; self.width];
            buf = vec![0; self.width];
            for &ch in possible.iter() {
                // Clear the big number and the buffer
                for x in 0 .. self.width {
                    big_number[x] = 0;
                    buf[x] = self.allowed_chars(x, y)[0];
                }
                // Put the character we're brute forcing in the right slot
                buf[x] = ch;
                // And... uh... start.
                let mut any_allowed = false;
                'trying_row: while !any_allowed {
                    // Try this string!
                    let as_str = unsafe { std::str::from_utf8_unchecked(&buf) };
                    let mut allowed = true;
                    if let Some(ref hint) = hints.left[y] {
                        if !hint.is_match(as_str).unwrap() {
                            allowed = false;
                        }
                    }
                    if allowed {
                        if let Some(ref hint) = hints.right[y] {
                            if !hint.is_match(as_str).unwrap() {
                                allowed = false;
                            }
                        }
                    }
                    if allowed {
                        any_allowed = true;
                        break;
                    }
                    else {
                        // Move on to the next possibility...
                        for i in 0 .. self.width {
                            if !open_cell[i] { continue }
                            let nu = big_number[i] + 1;
                            if nu >= possibilities[i].len() {
                                big_number[i] = 0;
                            }
                            else { big_number[i] = nu }
                            buf[i] = possibilities[i][big_number[i]];
                            if nu < possibilities[i].len() {
                                continue 'trying_row;
                            }
                        }
                        break 'trying_row; // we ran off the end of the big_number
                    }
                }
                if any_allowed {
                    really_possible.push(ch);
                }
            }
        }
        drop(possible);
        drop(possibilities);
        // now we don't have ourselves borrowed anymore...
        if really_possible.len() == possible.len() {
            // Add to blacklist, so that next time we try the next most complex thing
            self.print_cell(x, y, '?'); // cells we've tried but not yet resolved will look like non-dim ? now
            match self.progress_free_cells.binary_search(&(is_column, x, y)) {
                Ok(_) => panic!("If it were already there, why'd we try it!?"),
                Err(index) => {
                    self.progress_free_cells.insert(index, (is_column, x, y));
                },
            }
        }
        else {
            self.progress_free_cells.clear(); // no longer accurate!
            if really_possible.len() == 0 {
                print!("\x1B[33;7m");
                self.print_cell(x, y, '0');
                print!("\x1B[0m");
                let _ = std::io::stdout().flush();
                println!("Cell {},{} by {} ran out of possibilities!", x + 1, y + 1, if is_column { "column" } else { "row" });
                println!("Started with: {:?}", self.allowed_chars(x, y).iter().map(|&x| x as char).collect::<String>());
                std::process::exit(1);
            }
            else if really_possible.len() == 1 {
                self.undecided_cells.remove(index); // this is why we needed index
                print!("\x1B[1;32m");
                self.print_cell(x, y, really_possible[0] as char);
                print!("\x1B[0m");
            }
            else {
                self.print_cell(x, y, '?');
            }
            self.allowed_chars[x + y * self.width] = really_possible;
            // Now correct the whole row's (or column's) tree complexity, because
            // we have changed the values for everything in our row/column!
            if is_column {
                for y in 0 .. self.height {
                    self.recalculate_tree_complexity(x, y);
                }
            }
            else {
                for x in 0 .. self.width {
                    self.recalculate_tree_complexity(x, y);
                }
            }
        }
        // we didn't necessarily make progress, but we made progress toward
        // making progress!
        true
    }
    // This is NOT upside down because we know how tall we are
    fn print_cell(&self, x: usize, y: usize, wat: char) {
        let mut stdout = std::io::stdout();
        // (Assume the cursor is at the line after the board)
        // Save cursor position
        let _ = write!(stdout, "\x1B[s");
        // Move cursor up by (height - y) + 1
        let _ = write!(stdout, "\x1B[{}A", (self.height - y) + 1);
        // Move cursor right by x + 1
        let _ = write!(stdout, "\x1B[{}C", x + 1);
        // Output the given character
        let _ = write!(stdout, "{}", wat);
        // Restore cursor position
        let _ = write!(stdout, "\x1B[u");
        // Flush (so that the character will be displayed)
        stdout.flush().unwrap();
    }
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: {} puzzle.json",
                  args.get(0).map(String::as_str).unwrap_or("regex_solver"));
        std::process::exit(1);
    }
    let json = std::fs::read_to_string(&args[1])
        .context("Couldn't read puzzle file.")?;
    let spec: PuzzleSpec = serde_json::from_str(&json)
        .context("Couldn't parse puzzle file.")?;
    // This is the moment we decide we started working "on the puzzle"
    let start_time = Instant::now();
    let hints = Hints::from_spec(&spec)?;
    let all_allowed_chars = get_all_allowed_chars(&spec)?;
    println!("Here are all the allowed chars we found: {:?}",
             all_allowed_chars.iter().map(|x| *x as char).collect::<String>());
    let row_allowed_chars: Vec<Vec<u8>> = (0 .. spec.height).map(|y| {
        // For each row...
        let left_hint = spec.left_hints.as_ref().and_then(|x| x.get(y)).and_then(|x| x.as_ref());
        let right_hint = spec.right_hints.as_ref().and_then(|x| x.get(y)).and_then(|x| x.as_ref());
        get_both_allowed_chars(left_hint, right_hint, &all_allowed_chars).unwrap()
    }).collect();
    let col_allowed_chars: Vec<Vec<u8>> = (0 .. spec.width).map(|y| {
        // For each row...
        let top_hint = spec.top_hints.as_ref().and_then(|x| x.get(y)).and_then(|x| x.as_ref());
        let bottom_hint = spec.bottom_hints.as_ref().and_then(|x| x.get(y)).and_then(|x| x.as_ref());
        get_both_allowed_chars(top_hint, bottom_hint, &all_allowed_chars).unwrap()
    }).collect();
    println!("More finely:");
    for y in 0 .. spec.height {
        println!("  Row #{}: {:?}", y + 1, row_allowed_chars[y].iter().map(|&x| x as char).collect::<String>());
    }
    for x in 0 .. spec.width {
        println!("  Col #{}: {:?}", x + 1, col_allowed_chars[x].iter().map(|&x| x as char).collect::<String>());
    }
    // Print a board to put characters on
    print!("╔");
    for _ in 0 .. spec.width { print!("═") }
    print!("╗\n");
    for _ in 0 .. spec.height {
        print!("║\x1B[2m");
        for _ in 0 .. spec.width {
            print!("?");
        }
        print!("\x1B[0m║\n");
    }
    print!("╚");
    for _ in 0 .. spec.width { print!("═") }
    print!("╝\n");
    let mut board = Board::new(spec.width, spec.height, row_allowed_chars, col_allowed_chars);
    while board.still_undecided() {
        if !board.make_progress(&hints) {
            return Err(anyhow!("We couldn't make any more progress. Stumped!"));
        }
    }
    let solve_time = Instant::now();
    println!("Solved it! In {:.6} seconds!", (solve_time - start_time).as_secs_f32());
    Ok(())
}

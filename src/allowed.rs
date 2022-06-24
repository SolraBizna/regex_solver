//! Utilities for finding out what characters are allowed by parsing regexes.
use super::*;

fn byte_from_literal(literal: &regex_syntax::ast::Literal) -> u8 {
    let c = literal.c;
    if c < ' ' {
        panic!("ASCII control character in regex!!!");
    }
    else if c >= '\u{7F}' {
        panic!("Non-ASCII-printable-character in regex!!!!!");
    }
    c as u8 // safe because we excluded "high bytes"
}

fn add_all_literal(result: &mut Vec<u8>, literal: &regex_syntax::ast::Literal) {
    let c = byte_from_literal(literal);
    if !result.contains(&c) {
        result.push(c);
    }
}

fn add_literal(result: &mut Vec<u8>, literal: &regex_syntax::ast::Literal, all_allowed_chars: &[u8]) {
    let c = byte_from_literal(literal);
    if !result.contains(&c) && all_allowed_chars.contains(&c) {
        result.push(c);
    }
}

fn allowed_chars_for_perlkind(kind: &regex_syntax::ast::ClassPerlKind) -> &'static [u8] {
    use regex_syntax::ast::ClassPerlKind;
    match kind {
        ClassPerlKind::Digit => b"0123456789", // "\d"
        ClassPerlKind::Space => b" ", // "\s"
        ClassPerlKind::Word => b"ABCDEFGHIJKLMNOPQRSTUVWXYZ", // "\w"
    }
}

fn add_allowed_chars_from_class_set_item(result: &mut Vec<u8>, item: &regex_syntax::ast::ClassSetItem, all_allowed_chars: &[u8]) {
    use regex_syntax::ast::ClassSetItem;
    match item {
        ClassSetItem::Empty(..) => (),
        ClassSetItem::Literal(literal)
        => add_all_literal(result, &literal),
        ClassSetItem::Range(range) => {
            let start = byte_from_literal(&range.start);
            let end = byte_from_literal(&range.end);
            assert!(end >= start);
            for b in start ..= end {
                if !result.contains(&b) && all_allowed_chars.contains(&b) { result.push(b) }
            }
        },
        ClassSetItem::Ascii(..) => panic!("No ASCII classes in regex crossword ALLOWED!"), // e.g. "[[:alnum:][:digit:]]"
        ClassSetItem::Unicode(..) => panic!("HEY! NO UNICODE CLASSES EVEN IN SETS!"),
        ClassSetItem::Perl(perl) => {
            let allowed: &'static [u8] = allowed_chars_for_perlkind(&perl.kind);
            for &b in allowed.iter() {
                if !result.contains(&b) && all_allowed_chars.contains(&b)  { result.push(b) }
            }
        },
        ClassSetItem::Bracketed(..) => panic!("No nesting of brackets in brackets! That's too brackish!"),
        ClassSetItem::Union(union) => { // "[ab-dz]" is a union of "[a]", "[b-d]", and "[z]"
            for item in union.items.iter() {
                add_allowed_chars_from_class_set_item(result, item, all_allowed_chars);
            }
        },
    }
}

fn add_all_allowed_chars_from_class_set_item(result: &mut Vec<u8>, item: &regex_syntax::ast::ClassSetItem) {
    use regex_syntax::ast::ClassSetItem;
    match item {
        ClassSetItem::Empty(..) => (),
        ClassSetItem::Literal(literal)
        => add_all_literal(result, &literal),
        ClassSetItem::Range(range) => {
            let start = byte_from_literal(&range.start);
            let end = byte_from_literal(&range.end);
            assert!(end >= start);
            for b in start ..= end {
                if !result.contains(&b) { result.push(b) }
            }
        },
        ClassSetItem::Ascii(..) => panic!("No ASCII classes in regex crossword ALLOWED!"), // e.g. "[[:alnum:][:digit:]]"
        ClassSetItem::Unicode(..) => panic!("HEY! NO UNICODE CLASSES EVEN IN SETS!"),
        ClassSetItem::Perl(perl) => {
            let allowed: &'static [u8] = allowed_chars_for_perlkind(&perl.kind);
            for &b in allowed.iter() {
                if !result.contains(&b) { result.push(b) }
            }
        },
        ClassSetItem::Bracketed(..) => panic!("No nesting of brackets in brackets! That's too brackish!"),
        ClassSetItem::Union(union) => { // "[ab-dz]" is a union of "[a]", "[b-d]", and "[z]"
            for item in union.items.iter() {
                add_all_allowed_chars_from_class_set_item(result, item);
            }
        },
    }
}

fn add_all_allowed_chars_from_ast(result: &mut Vec<u8>, ast: &Ast) {
    use Ast::*;
    use regex_syntax::ast::Class;   
    match ast {
        // "a|b"
        Alternation(alternation) => {
            for ast in alternation.asts.iter() {
                add_all_allowed_chars_from_ast(result, ast);
            }
        },
        // "\s", "\p{Greek}", "[a-xz]"
        Class(class) => match class {
            Class::Unicode(_unicode) => {
                panic!("No unicode classes in regex crossword ALLOWED!")
            },
            Class::Perl(perl) => {
                // "\s" = only space, "\S" = anything BUT space
                if !perl.negated {
                    let allowed: &'static [u8] = allowed_chars_for_perlkind(&perl.kind);
                    for &b in allowed.iter() {
                        if !result.contains(&b) { result.push(b) }
                    }
                }
            },
            Class::Bracketed(bracketed) => {
                if !bracketed.negated {
                    use regex_syntax::ast::ClassSet;
                    match &bracketed.kind {
                        ClassSet::Item(item) => {
                            add_all_allowed_chars_from_class_set_item(result, &item);
                        },
                        ClassSet::BinaryOp(..) => {
                            panic!("No binary ops in regex crossword ALLOWED!");
                        },
                    }
                }
            },
        },
        // "abc" "(a)d(g)"
        Concat(concat) => {
            for ast in concat.asts.iter() {
                add_all_allowed_chars_from_ast(result, ast);
            }
        },
        // "(a)"
        Group(group) => {
            add_all_allowed_chars_from_ast(result, group.ast.as_ref());   
        },
        // "a"
        Literal(literal) => add_all_literal(result, literal),
        // "a{1,3}" "a?" "a*" "a+"
        Repetition(repetition) => {
            add_all_allowed_chars_from_ast(result, repetition.ast.as_ref());
        },
        // and the ignored cases
        // ""
        Empty(..) => (),
        // the "i" in "(?i)"
        Flags(..) => (),
        // "."
        Dot(..) => (),
        // "^" "$"
        Assertion(..) => (),
    }
}

fn add_allowed_chars_from_ast(result: &mut Vec<u8>, ast: &Ast, all_allowed_chars: &[u8]) {
    use Ast::*;
    use regex_syntax::ast::Class;   
    match ast {
        // "a|b"
        Alternation(alternation) => {
            for ast in alternation.asts.iter() {
                add_allowed_chars_from_ast(result, ast, all_allowed_chars);
            }
        },
        // "\s", "\p{Greek}", "[a-xz]"
        Class(class) => match class {
            Class::Unicode(_unicode) => {
                panic!("No unicode classes in regex crossword ALLOWED!")
            },
            Class::Perl(perl) => {
                // "\s" = only space, "\S" = anything BUT space
                if !perl.negated {
                    let allowed: &'static [u8] = allowed_chars_for_perlkind(&perl.kind);
                    for &b in allowed.iter() {
                        if !result.contains(&b) && all_allowed_chars.contains(&b) { result.push(b) }
                    }
                }
                else {
                    result.clear();
                    result.extend_from_slice(all_allowed_chars);
                }
            },
            Class::Bracketed(bracketed) => {
                if !bracketed.negated {
                    use regex_syntax::ast::ClassSet;
                    match &bracketed.kind {
                        ClassSet::Item(item) => {
                            add_allowed_chars_from_class_set_item(result, &item, all_allowed_chars);
                        },
                        ClassSet::BinaryOp(..) => {
                            panic!("No binary ops in regex crossword ALLOWED!");
                        },
                    }
                }
                else {
                    result.clear();
                    result.extend_from_slice(all_allowed_chars);
                }
            },
        },
        // "abc" "(a)d(g)"
        Concat(concat) => {
            for ast in concat.asts.iter() {
                add_allowed_chars_from_ast(result, ast, all_allowed_chars);
            }
        },
        // "."
        Dot(..) => {
            // It would be more efficient if we would bail out of any subsequent
            // parsing of the AST at this point, but that's too much work and
            // it will be a tiny portion of our runtime anyway.
            result.clear();
            result.extend_from_slice(all_allowed_chars);
        },
        // "(a)"
        Group(group) => {
            add_allowed_chars_from_ast(result, group.ast.as_ref(), all_allowed_chars);   
        },
        // "a"
        Literal(literal) => add_literal(result, literal, all_allowed_chars),
        // "a{1,3}" "a?" "a*" "a+"
        Repetition(repetition) => {
            add_allowed_chars_from_ast(result, repetition.ast.as_ref(), all_allowed_chars);
        },
        // and the ignored cases
        // ""
        Empty(..) => (),
        // the "i" in "(?i)"
        Flags(..) => (),
        // "^" "$"
        Assertion(..) => (),
    }
}

fn add_all_allowed_chars(result: &mut Vec<u8>, hints: Option<&Vec<Option<String>>>) {
    if let Some(hints) = hints {
        for hint in hints.iter() {
            if let Some(hint) = hint {
                // A hint! A very palpable hint!
                // The regex-syntax crate doesn't support backreferences.
                // Fortunately, we can ignore backreferences, since all we're
                // doing is parsing what characters are allowed, and
                // backreferences can't add to that set!
                let stripped_hint = BACKREFERENCE_STRIPPING_REGEX.replace_all(hint, "");
                let mut parser = regex_syntax::ast::parse::Parser::new();
                let ast = parser.parse(&stripped_hint).unwrap();
                add_all_allowed_chars_from_ast(result, &ast);
            }
        }
    }
}

pub(crate) fn get_all_allowed_chars(spec: &PuzzleSpec) -> anyhow::Result<Vec<u8>> {
    let mut result = vec![];
    add_all_allowed_chars(&mut result, spec.top_hints.as_ref());
    add_all_allowed_chars(&mut result, spec.bottom_hints.as_ref());
    add_all_allowed_chars(&mut result, spec.left_hints.as_ref());
    add_all_allowed_chars(&mut result, spec.right_hints.as_ref());
    result.sort();
    Ok(result)
}

fn get_allowed_chars(hint: &str, all_allowed_chars: &[u8]) -> anyhow::Result<Vec<u8>> {
    let stripped_hint = BACKREFERENCE_STRIPPING_REGEX.replace_all(hint, "");
    let mut parser = regex_syntax::ast::parse::Parser::new();
    let ast = parser.parse(&stripped_hint).unwrap();
    let mut result = Vec::with_capacity(all_allowed_chars.len());
    add_allowed_chars_from_ast(&mut result, &ast, all_allowed_chars);
    result.sort();
    Ok(result)
}

pub fn allowed_char_intersection(mut a: &[u8], mut b: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(a.len().min(b.len()));
    while !a.is_empty() && !b.is_empty() {
        let ac = a[0];
        let bc = b[0];
        if ac == bc {
            result.push(ac);
            a = &a[1..];
            b = &b[1..];
        }
        else if ac < bc {
            a = &a[1..];
        }
        else if bc < ac {
            b = &b[1..];
        }
    }
    result
}

pub(crate) fn get_both_allowed_chars(hint_a: Option<&String>, hint_b: Option<&String>, all_allowed_chars: &[u8]) -> anyhow::Result<Vec<u8>> {
    match (hint_a, hint_b) {
        (None, None) => panic!("No hint for this row/column!?"),
        (Some(hint), None) | (None, Some(hint)) => get_allowed_chars(hint, all_allowed_chars),
        (Some(hint_a), Some(hint_b)) => {
            let result_a = get_allowed_chars(hint_a, all_allowed_chars)?;
            let result_b = get_allowed_chars(hint_b, all_allowed_chars)?;
            Ok(allowed_char_intersection(&result_a, &result_b))
        },
    }
}


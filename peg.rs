use codegen::RustWriter;
use std::io;
use std::str;
mod codegen;

struct Grammar {
	initializer: Option<~str>,
	rules: ~[~Rule]
}

enum Item {
	Rule
}

struct Rule {
	name: ~str,
	expr: ~Expr,
	ret_type: ~str,
}

enum Attr {
	AttrDefineParser(~str),
	AttrDefineType(~str)
}

struct CharSetCase {
	start: char,
	end: char
}

struct TaggedExpr {
	name: Option<~str>,
	expr: ~Expr
}

enum Expr {
	AnyCharExpr,
	LiteralExpr(~str),
	CharSetExpr(bool, ~[CharSetCase]),
	RuleExpr(~str),
	SequenceExpr(~[~Expr]),
	ChoiceExpr(~[~Expr]),
	OptionalExpr(~Expr),
	ZeroOrMore(~Expr),
	OneOrMore(~Expr),
	DelimitedExpr(~Expr, ~Expr),
	PosAssertExpr(~Expr),
	NegAssertExpr(~Expr),
	StringifyExpr(~Expr),
	ActionExpr(~[TaggedExpr], ~str),
}

fn compile_grammar(w: &RustWriter, grammar: &Grammar) {
	compile_header(w);

	match grammar.initializer {
		Some(ref initializer) => {
			w.write(*initializer);
		}
		_ => ()
	}

	for rule in grammar.rules.iter() {
		compile_rule(w, *rule);
	}
}



fn compile_header(w: &RustWriter) {
	w.write("// Generated by rust-peg. Do not edit.
extern mod std;
use std::str::{CharRange};

#[inline]
fn slice_eq(input: &str, pos: uint, m: &str) -> Result<(uint, ()), uint> {
    let l = m.len();
    if (input.len() >= pos + l && input.slice(pos, pos+l) == m) {
        Ok((pos+l, ()))
    } else {
        Err(pos)
    }
}

#[inline]
fn any_char(input: &str, pos: uint) -> Result<(uint, ()), uint> {
    if input.len() > pos {
        Ok((input.char_range_at(pos).next, ()))
    } else {
        Err(pos)
    }
}
");
}


fn compile_rule(w: &RustWriter, rule: &Rule) {
	w.line("#[allow(unused_variable)]");
	do w.def_fn(false, "parse_"+rule.name, "input: &str, pos: uint", "Result<(uint, " + rule.ret_type + ") , uint>") {
		compile_expr(w, rule.expr, rule.ret_type != ~"()");
	}

	do w.def_fn(true, rule.name, "input: &str", "Result<"+rule.ret_type+", ~str>") {
		do w.match_block("parse_"+rule.name+"(input, 0)") {
			do w.match_case("Ok((pos, value))") {
				w.if_else("pos == input.len()",
					|| { w.line("Ok(value)"); },
					|| { w.line("Err(~\"Unexpected characters at end of input\")"); }
				)
			}
			w.match_inline_case("Err(pos)", "Err(\"Error at \"+ pos.to_str())");
		}
	}
}

fn compile_match_and_then(w: &RustWriter, e: &Expr, value_name: Option<&str>, then: &fn()) {
	do w.let_block("seq_res") {
		compile_expr(w, e, value_name.is_some());
	}
	do w.match_block("seq_res") {
		w.match_inline_case("Err(pos)", "Err(pos)");
		do w.match_case("Ok((pos, "+value_name.unwrap_or_default("_")+"))") {
			then();
		}
	}
}

fn compile_zero_or_more(w: &RustWriter, e: &Expr, list_initial: Option<&str>) {
	w.let_mut_stmt("repeat_pos", "pos");
	let result_used = list_initial.is_some();
	if (result_used) {
		w.let_mut_stmt("repeat_value", list_initial.unwrap());
	}
	do w.loop_block {
		do w.let_block("step_res") {
			w.let_stmt("pos", "repeat_pos");
			compile_expr(w, e, result_used);
		}
		do w.match_block("step_res") {
			let match_arm = if result_used {
				"Ok((newpos, value))"
			} else {
				"Ok((newpos, _))"
			};
			do w.match_case(match_arm) {
				w.line("repeat_pos = newpos;");
				if result_used {
					w.line("repeat_value.push(value);");
				}
			}
			do w.match_case("Err(*)") {
				w.line("break;");
			}
		}
	}
	if result_used {
		w.line("Ok((repeat_pos, repeat_value))");
	} else {
		w.line("Ok((repeat_pos, ()))");
	}
}

fn compile_expr(w: &RustWriter, e: &Expr, result_used: bool) {
	match *e {
		AnyCharExpr => { 
			w.line("any_char(input, pos)");
			/*w.if_else("input.len() > pos",
				||{ w.line("Ok(pos+1)"); },
				||{ w.line("Err(pos)"); }
			);*/
		}

		LiteralExpr(ref s) => {
			w.line("slice_eq(input, pos, \""+*s+"\")");
			/*w.if_else("slice_eq(input, pos, \""+*s+"\")",
				||{ w.line("Ok(pos+" + s.len().to_str() + ")"); },
				||{ w.line("Err(pos)"); }
			);*/
		}

		CharSetExpr(invert, ref cases) => {
			let result_strs = ("Ok((next, ()))", "Err(pos)");
			let (y_str, n_str) = if !invert { result_strs } else { result_strs.swap() };

			w.if_else("input.len() > pos",
				|| {
					w.line("let CharRange {ch, next} = input.char_range_at(pos);");
					do w.match_block("ch") {
						w.write_indent();
						for (i, case) in cases.iter().enumerate() {
							if i != 0 { w.write(" | "); }
							if case.start == case.end {
								w.write("'"+str::from_char(case.start)+"'");
							} else {
								w.write("'"+str::from_char(case.start)+"'..'"+str::from_char(case.end)+"'");
							}
						}
						w.write(" => { "+y_str+" }\n");
						w.match_inline_case("_", n_str);
					}
				},
				|| { w.line("Err(pos)"); }
			)
		}
		
		RuleExpr(ref ruleName) => {
			w.line("parse_"+*ruleName+"(input, pos)");
		}

		SequenceExpr(ref exprs) => {
			fn write_seq(w: &RustWriter, exprs: &[~Expr]) {
				if (exprs.len() == 1) {
					compile_expr(w, exprs[0], false);
				} else {
					do compile_match_and_then(w, exprs[0], None) {
						write_seq(w, exprs.tail());
					}
				}
			}

			if (exprs.len() > 0 ) {
				write_seq(w, *exprs);
			}
		}

		ChoiceExpr(ref exprs) => {
			fn write_choice(w: &RustWriter, exprs: &[~Expr], result_used: bool) {
				if (exprs.len() == 1) {
					compile_expr(w, exprs[0], result_used);
				} else {
					do w.let_block("choice_res") {
						compile_expr(w, exprs[0], result_used);
					}
					do w.match_block("choice_res") {
						w.match_inline_case("Ok((pos, value))", "Ok((pos, value))");
						do w.match_case("Err(*)") {
							write_choice(w, exprs.tail(), result_used);
						}
					}
				}
			}

			if (exprs.len() > 0 ) {
				write_choice(w, *exprs, result_used);
			}
		}

		OptionalExpr(ref e) => {
			do w.let_block("optional_res") {
				compile_expr(w, *e, result_used);
			}
			do w.match_block("optional_res") {
				w.match_inline_case("Ok((newpos, value))", "Ok((newpos, Some(value)))");
				w.match_inline_case("Err(*)", "Ok((pos, None))");
			}
		}
		
		ZeroOrMore(ref e) => {
			compile_zero_or_more(w, *e, if result_used { Some("~[]") } else { None });
		}

		OneOrMore(ref e) => {
			do compile_match_and_then(w, *e, if result_used { Some("first_value") } else { None }) {
				compile_zero_or_more(w, *e, if result_used { Some("~[first_value]") } else { None });
			}
		}
		
		DelimitedExpr(_, _) => fail!("not implemented"),
		StringifyExpr(*) => fail!("not implemented"),

		PosAssertExpr(ref e) => {
			do w.let_block("assert_res") {
				compile_expr(w, *e, false);
			}
			do w.match_block("assert_res") {
				w.match_inline_case("Ok(*)", "Ok((pos, ()))");
				w.match_inline_case("Err(*)", "Err(pos)");
			}
		}

		NegAssertExpr(ref e) => {
			do w.let_block("neg_assert_res") {
				compile_expr(w, *e, false);
			}
			do w.match_block("neg_assert_res") {
				w.match_inline_case("Err(*)", "Ok((pos, ()))");
				w.match_inline_case("Ok(*)", "Err(pos)");
			}
		}

		ActionExpr(ref exprs, ref code) => {
			w.let_stmt("start_pos", "pos");
			fn write_seq(w: &RustWriter, exprs: &[TaggedExpr], code: &str) {
				if (exprs.len() > 0) {
					let name = exprs.head().name.map(|s| s.as_slice());
					do compile_match_and_then(w, exprs.head().expr, name) {
						write_seq(w, exprs.tail(), code);
					}
				} else {
					w.let_stmt("match_str",  "input.slice(start_pos, pos);");
					w.write_indent();
					w.write("Ok((pos, (|| {");
					w.write(code);
					w.write("})()))\n");
				}
			}

			write_seq(w, *exprs, *code);
		}
	}
}

fn main() {
	let grammar = include!("grammar_def.rs");
	let w = RustWriter::new(io::stdout());
	compile_grammar(&w, grammar);
}
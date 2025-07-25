use crate::token::{Token, Kind};

macro_rules! token {
    ($self: ident, $kind: expr) => {
        Token { kind: $kind, position: $self.yybytepos() }
    }
}

macro_rules! parse_number {
    ($self: ident, $suffix_len: expr, $parser: expr) => {{
        let mut text = $self.yytext();
        let drain = text.drain(0..(text.len() - $suffix_len));
        let s = drain.collect::<String>();
        $parser(s)
    }}
}

%%
%class Lexer
%result_type Token
%field u64 line_count

"if"        return Ok(token!(self, Kind::If));
"elif"      return Ok(token!(self, Kind::Elif));
"else"      return Ok(token!(self, Kind::Else));
"for"       return Ok(token!(self, Kind::For));
"in"        return Ok(token!(self, Kind::In));
"to"        return Ok(token!(self, Kind::To));
"while"     return Ok(token!(self, Kind::While));
"break"     return Ok(token!(self, Kind::Break));
"continue"  return Ok(token!(self, Kind::Continue));
"return"    return Ok(token!(self, Kind::Return));

"class"     return Ok(token!(self, Kind::Class));
"struct"    return Ok(token!(self, Kind::Struct));
"impl"      return Ok(token!(self, Kind::Impl));
"fn"        return Ok(token!(self, Kind::Function));
"extern"    return Ok(token!(self, Kind::Extern));
"pub"       return Ok(token!(self, Kind::Public));
"val"       return Ok(token!(self, Kind::Val));
"var"       return Ok(token!(self, Kind::Var));

"true"      return Ok(token!(self, Kind::True));
"false"     return Ok(token!(self, Kind::False));

"u64"       return Ok(token!(self, Kind::U64));
"i64"       return Ok(token!(self, Kind::I64));
"str"       return Ok(token!(self, Kind::Str));
"ptr"       return Ok(token!(self, Kind::Ptr));
"usize"     return Ok(token!(self, Kind::USize));
"null"      return Ok(token!(self, Kind::Null));
"bool"      return Ok(token!(self, Kind::Bool));

"::"        return Ok(token!(self, Kind::DoubleColon));
"->"        return Ok(token!(self, Kind::Arrow));
"=="        return Ok(token!(self, Kind::DoubleEqual));
"!="        return Ok(token!(self, Kind::NotEqual));
"<="        return Ok(token!(self, Kind::LE));
">="        return Ok(token!(self, Kind::GE));
"&&"        return Ok(token!(self, Kind::DoubleAnd));
"||"        return Ok(token!(self, Kind::DoubleOr));

"("         return Ok(token!(self, Kind::ParenOpen));
")"         return Ok(token!(self, Kind::ParenClose));
"{"         return Ok(token!(self, Kind::BraceOpen));
"}"         return Ok(token!(self, Kind::BraceClose));
"["         return Ok(token!(self, Kind::BracketOpen));
"]"         return Ok(token!(self, Kind::BracketClose));
","         return Ok(token!(self, Kind::Comma));
"."         return Ok(token!(self, Kind::Dot));
":"         return Ok(token!(self, Kind::Colon));
";"         return Ok(token!(self, Kind::Semicolon));
"_"         return Ok(token!(self, Kind::Underscore));
"!"         return Ok(token!(self, Kind::Exclamation));
"="         return Ok(token!(self, Kind::Equal));
"<"         return Ok(token!(self, Kind::LT));
">"         return Ok(token!(self, Kind::GT));
"&"         return Ok(token!(self, Kind::And));
"+"         return Ok(token!(self, Kind::IAdd));
"-"         return Ok(token!(self, Kind::ISub));
"*"         return Ok(token!(self, Kind::IMul));
"/"         return Ok(token!(self, Kind::IDiv));


-?[0-9]+"i64"       return Ok(token!(self, Kind::Int64(
                        parse_number!(self, 3, |s: String| s.parse::<i64>().unwrap())
                    )));

[0-9]+"u64"         return Ok(token!(self, Kind::UInt64(
                        parse_number!(self, 3, |s: String| s.parse::<u64>().unwrap())
                    )));

-?[0-9]+            return Ok(token!(self, Kind::Integer(self.yytext())));

\".*\"              {
                        let mut text = self.yytext();
                        let drain = text.drain(1..(text.len()-1));
                        return Ok(token!(self, Kind::String(drain.collect::<String>())));
                    }

[A-Za-z_][A-Za-z_0-9]*  return Ok(token!(self, Kind::Identifier(self.yytext())));

"#".*               {
                        let mut text = self.yytext();
                        let drain = text.drain(1..);
                        return Ok(token!(self, Kind::Comment(drain.collect::<String>())));
                    }

" "                 /* skip whitespace */
\t                  /* skip tab */
\n                  self.line_count += 1; return Ok(token!(self, Kind::NewLine));

%%

    pub fn get_current_line_count(&self) -> usize {
        self.line_count as usize
    }
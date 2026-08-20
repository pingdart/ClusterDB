use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser;

fn main() {
    let sql = "UPDATE users SET name = 'Bob', age = 30 WHERE id = 1;";
    let sql2 = "DELETE FROM users WHERE id = 2;";
    let dialect = GenericDialect {};
    
    let ast1 = Parser::parse_sql(&dialect, sql).unwrap();
    println!("UPDATE: {:#?}", ast1);
    
    let ast2 = Parser::parse_sql(&dialect, sql2).unwrap();
    println!("DELETE: {:#?}", ast2);
}

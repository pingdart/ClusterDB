use sqlparser::dialect::MySqlDialect;
use sqlparser::parser::Parser;
fn main() {
    let sql = "CREATE DATABASE IF NOT EXISTS testdb; USE testdb; CREATE INDEX idx ON products(name);";
    let dialect = MySqlDialect {};
    let ast = Parser::parse_sql(&dialect, sql).unwrap();
    println!("{:#?}", ast);
}

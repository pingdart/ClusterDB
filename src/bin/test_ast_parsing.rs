use sqlparser::dialect::MySqlDialect;
use sqlparser::parser::Parser;
fn main() {
    let sql = "CREATE TABLE doctors (id INT, bio TEXT, embedding VECTOR(384) GENERATED ALWAYS AS (bio) COMMENT 'fast_embed');";
    let dialect = MySqlDialect {};
    match Parser::parse_sql(&dialect, sql) {
        Ok(ast) => println!("{:#?}", ast),
        Err(e) => println!("Error: {:?}", e),
    }
}

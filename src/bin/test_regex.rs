fn main() {
    let re = regex::Regex::new(r"\s{3,}\d+$").unwrap();
    println!("Matches: {}", re.is_match("AT 1 Vorbemerkung     6"));
    println!("Matches: {}", re.is_match("AT 1 Vorbemerkung 6"));
    println!("Matches: {}", re.is_match("normal sentence with 6"));
}

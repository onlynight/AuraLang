use object::read::File as ObjectFile;
use object::read::Object;
use object::read::ObjectSymbol;

fn main() {
    let bytes = std::fs::read("D:\\Code\\AuraLang\\test_export.dll").unwrap();
    let file = ObjectFile::parse(&bytes[..]).unwrap();

    println!("File kind: {:?}", file.kind());

    let mut count = 0;
    let mut found = false;
    for symbol in file.symbols() {
        count += 1;
        let name = symbol.name().unwrap_or("<error>");
        if count <= 20 {
            println!(
                "Symbol[{}]: {} (is_undefined={})",
                count,
                name,
                symbol.is_undefined()
            );
        }
        if name.starts_with("aura_aot_") {
            println!("FOUND: {}", name);
            found = true;
        }
    }

    println!("Total symbols: {}", count);
    if !found {
        println!("No aura_aot_* symbols found!");
    }
}

import re

with open("/Users/matthias.nordwig/matrix/core/src/db/mod.rs", "r") as f:
    text = f.read()

# Find the match and append the v9 migration
pattern = r'include_str\!\("schema_v8\.sql"\)\s*\];'
new_text = 'include_str!("schema_v8.sql"),\n        include_str!("schema_v9.sql")\n    ];'

text = re.sub(pattern, new_text, text)

with open("/Users/matthias.nordwig/matrix/core/src/db/mod.rs", "w") as f:
    f.write(text)

print("mod.rs patched")

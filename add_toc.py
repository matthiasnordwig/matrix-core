import sqlite3

db_path = "/Users/matthias.nordwig/Library/Application Support/com.matthiasnordwig.matrix/matrix.db"
conn = sqlite3.connect(db_path)
cur = conn.cursor()

# Find the structural profile (assuming ID 1 or the first one)
cur.execute("SELECT id FROM structural_profiles ORDER BY id LIMIT 1")
row = cur.fetchone()
if row:
    profile_id = row[0]
    
    # Check if the TOC pattern already exists
    cur.execute("SELECT id FROM structural_patterns WHERE profile_id = ? AND role = 'ignore'", (profile_id,))
    if not cur.fetchone():
        cur.execute("""
            INSERT INTO structural_patterns (profile_id, group_name, role, regex, flags, priority, sort_order)
            VALUES (?, 'Inhaltsverzeichnis', 'ignore', '\\s{3,}\\d+$', 'm', 10, 3)
        """, (profile_id,))
        conn.commit()
        print("Successfully added TOC pattern to database!")
    else:
        print("TOC pattern already exists!")
else:
    print("No structural profile found in DB.")

conn.close()

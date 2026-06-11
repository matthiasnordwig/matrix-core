with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "r") as f:
    lines = f.readlines()

new_lines = []
skip = False
for i, line in enumerate(lines):
    if '<label className="inline">Chunk with:' in line:
        skip = True
    if skip and '</label>' in line:
        skip = False
        continue
    if not skip:
        new_lines.append(line)

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "w") as f:
    f.writelines(new_lines)

print("done")

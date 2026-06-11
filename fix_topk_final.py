import re

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "r") as f:
    lines = f.readlines()

for i in range(120, 130):
    if "setTopK" in lines[i]:
        lines[i] = ""

count = 0
for i in range(800, 900):
    if "const [topK, setTopK]" in lines[i]:
        count += 1
        if count > 1:
            lines[i] = ""

for i in range(950, 1050):
    if "const [topK, setTopK]" in lines[i]:
        lines[i] = "  const [topK, setTopK] = useState(5);\n"

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "w") as f:
    f.writelines(lines)
print("fixed")

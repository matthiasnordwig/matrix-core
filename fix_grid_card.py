with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "r") as f:
    text = f.read()

text = text.replace('        <table className="grid">', '      <div className="card">\n        <table className="grid">')

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "w") as f:
    f.write(text)
print("fixed grid card")

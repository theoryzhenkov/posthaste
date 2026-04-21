# Serve docs locally with live reload
serve *args:
    mkdocs serve {{args}}

# Build docs into site/
build *args:
    mkdocs build --strict {{args}}

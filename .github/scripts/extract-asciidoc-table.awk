BEGIN {
  FS = "\t"
  OFS = "\t"
}

function trim(value) {
  sub(/^[[:space:]]+/, "", value)
  sub(/[[:space:]]+$/, "", value)
  return value
}

function finish_cell() {
  if (!have_cell) {
    return
  }
  cells[++cell_count] = trim(cell)
  cell = ""
  have_cell = 0
  if (cell_count == columns) {
    for (column_index = 1; column_index <= columns; column_index++) {
      printf "%s%s", cells[column_index], column_index == columns ? ORS : OFS
      delete cells[column_index]
    }
    cell_count = 0
  }
}

/^\|===[[:space:]]*$/ {
  if (in_table) {
    finish_cell()
    cell_count = 0
    in_table = 0
  } else {
    in_table = 1
  }
  next
}

in_table && /^[[:alpha:]]*\|/ {
  finish_cell()
  cell = $0
  sub(/^[[:alpha:]]*\|/, "", cell)
  have_cell = 1
  next
}

in_table && have_cell {
  continuation = trim($0)
  if (continuation != "") {
    cell = cell " " continuation
  }
}

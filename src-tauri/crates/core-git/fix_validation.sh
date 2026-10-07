#!/bin/bash

# Fix all for loop comparisons
while IFS= read -r line; do
  line_no=$(echo "$line" | cut -d: -f2)
  if [[ "$line" =~ "error" ]] && [[ "$line" =~ "validation.rs" ]]; then
    # Extract line number
    linenum=$(echo "$line" | grep -o '[0-9]\+:' | head -1 | tr -d ':')
    if [ -n "$linenum" ]; then
      # Check if it's in a for loop by looking backwards
      # For now, just add * before arg
      sed -i "${linenum}s/if arg == /if *arg == /g" src/validation.rs
      sed -i "${linenum}s/\|\| arg == /|| *arg == /g" src/validation.rs
    fi
  fi
done < <(cargo test -p core-git --lib 2>&1 | grep "error\[E0277\]")

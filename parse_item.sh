#!/usr/bin/env bash
#
# Usage: ./parse_item.sh <value>
# <value> 可以是十六进制（带或不带 0x 前缀）或十进制，如： 0x5A9FD0 或 5840464

val="$1"
if [[ -z "$val" ]]; then
  echo "Usage: $0 <value>"
  exit 1
fi

# 支持十六进制前缀 0x 或 16# 表示法
if [[ "$val" =~ ^0x ]]; then
  num=$((val))
elif [[ "$val" =~ ^[0-9A-Fa-f]+$ ]]; then
  num=$((16#$val))
else
  num=$((val))
fi

# 按 bitfield 解析
lp_len=$(( num & 0x7FFF ))
lp_flags=$(( (num >> 15) & 0x3 ))
lp_off=$(( (num >> 17) & 0x7FFF ))

printf "Parsed value: %s (decimal: %d)\n" "$val" "$num"
printf "lp_len   = %d\n" "$lp_len"
printf "lp_flags = %d\n" "$lp_flags"
printf "lp_off   = %d\n" "$lp_off"
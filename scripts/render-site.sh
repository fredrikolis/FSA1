#!/usr/bin/env bash
# Concern: fills index.html's fsa1 regions | Non-concern: the page's hand-written markup, the fixture | IO: (index.html, website/fixture, the CLI) -> itself; 1 on drift
set -euo pipefail
# ls order and the [A-Za-z] class are collation-dependent; the page is bytes, so pin the locale.
export LC_ALL=C

cd "$(git rev-parse --show-toplevel)"

PAGE=website/src/index.html
FIXTURE=website/fixture
SPEC_TAG='<script class="fsa1-spec" type="application/json">'
NL='
'

check=0
case "${1:-}" in
	'') ;;
	--check) check=1 ;;
	*) printf 'usage: render-site.sh [--check]\n' >&2; exit 2 ;;
esac

if [ -n "${FSA1_CLI:-}" ]; then
	CLI=$FSA1_CLI
else
	cargo build --locked -p fsa1-cli >&2
	CLI=${CARGO_TARGET_DIR:-target}/debug/fsa1-cli
fi
case $CLI in /*) ;; *) CLI=$PWD/$CLI ;; esac
if [ ! -x "$CLI" ]; then
	printf 'render-site: not an executable: %s\n' "$CLI" >&2
	exit 1
fi

WORK=$(mktemp -d "${TMPDIR:-/tmp}/render-site.XXXXXX")
trap 'rm -rf "$WORK"' EXIT

refuse() {
	printf 'render-site: %s\n  in region <!--fsa1:%s-->\n' "$1" "$2" >&2
	exit 1
}

# argv is source text from the page, so it is split on spaces and never globbed.
run_cmd() {
	local w
	local -a cmd=()
	set -f
	for w in $1; do
		if [ ${#cmd[@]} -eq 0 ] && [ "$w" = fsa1-cli ]; then w=$CLI; fi
		cmd[${#cmd[@]}]=$w
	done
	set +f
	if [ ${#cmd[@]} -eq 0 ]; then return 2; fi
	( cd "$FIXTURE" && "${cmd[@]}" )
}

RKEY=
render_html() {
	if [ "$1" = "$RKEY" ]; then return 0; fi
	run_cmd "fsa1-cli render $1 --format html" >"$WORK/render.html" || return $?
	RKEY=$1
}

# Exits 3 when the anchor is absent, 4 when it appears more than once.
extract() {
	awk -v mode="$1" -v name="$2" -v tag="$SPEC_TAG" '
	function count(hay, ndl,   c, p) {
		c = 0
		while ((p = index(hay, ndl)) > 0) { c++; hay = substr(hay, p + length(ndl)) }
		return c
	}
	function span(hay, opener, closer,   s, e) {
		s = index(hay, opener)
		e = index(substr(hay, s), closer)
		if (s == 0 || e == 0) return ""
		return substr(hay, s, e + length(closer) - 1)
	}
	{ doc = doc $0 "\n" }
	END {
		if (mode == "sheet" || mode == "style") {
			n = count(doc, "<fsa1-caption>")
			if (n == 0) exit 3
			if (n > 1) exit 4
		}
		if (mode == "style") {
			head = substr(doc, 1, index(doc, "<fsa1-caption>") - 1)
			n = count(head, "<style>")
			if (n == 0 || index(head, "</style>") == 0) exit 3
			if (n > 1) exit 4
			printf "%s", span(head, "<style>", "</style>")
		} else if (mode == "sheet") {
			if (index(doc, "</fsa1-sheet>") == 0) exit 3
			printf "%s", span(doc, "<fsa1-caption>", "</fsa1-sheet>")
		} else {
			cap = "<figcaption>" name "</figcaption>"
			n = count(doc, cap)
			if (n == 0) exit 3
			if (n > 1) exit 4
			rest = substr(doc, index(doc, cap))
			s = index(rest, tag)
			if (s == 0 || index(substr(rest, 1, s), "</figure>") > 0) exit 3
			if (index(substr(rest, s), "</script>") == 0) exit 3
			printf "%s", span(substr(rest, s), tag, "</script>")
		}
	}
	' "$WORK/render.html"
}

# Not bash substitution: since 5.1 an `&` in the replacement stands for the match, and every
# entity starts with one.
esc() {
	printf '%s' "$1" | sed -e 's/&/\&amp;/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g'
}

# A run reaches the end of its line, or the `~` that opens a display format — whichever comes
# first. The format is the fx span's sibling: the page paints the two differently.
fx_line() {
	local line=$1 res= pre tail run fmt
	while :; do
		pre=${line%%=*}
		if [ "$pre" = "$line" ]; then res=$res$line; break; fi
		tail=${line#*=}
		case $tail in
			[A-Za-z]*) ;;
			*) res=$res$pre'='; line=$tail; continue ;;
		esac
		run="=$tail"
		fmt=
		case $run in *~*) fmt='~'${run#*~}; run=${run%%~*} ;; esac
		res=$res$pre'<span class="fx">'$run'</span>'
		if [ -n "$fmt" ]; then res=$res'<span class="fmt">'$fmt'</span>'; fi
		break
	done
	printf '%s' "$res"
}

decorate() {
	local text=$1 fx=$2 d=$3 line res= first=1 dline
	while IFS= read -r line; do
		if [ $first -eq 1 ]; then first=0; else res=$res$NL; fi
		dline=0
		case $line in ?*:) dline=1 ;; esac
		if [ "$d" = 1 ] && [ $dline -eq 1 ]; then
			res=$res'<span class="d">'$line'</span>'
		elif [ "$fx" = 1 ]; then
			res=$res$(fx_line "$line")
		else
			res=$res$line
		fi
	done <<<"$text"
	printf '%s' "$res"
}

anchored() {
	local mode=$1 name=$2 argv=$3 directive=$4 status=0
	render_html "$argv" || refuse "exited non-zero: fsa1-cli render $argv --format html" "$directive"
	payload=$(extract "$mode" "$name") || status=$?
	case $status in
		0) ;;
		3) refuse "no $mode anchor in the output of: fsa1-cli render $argv --format html" "$directive" ;;
		4) refuse "the $mode anchor appears more than once in: fsa1-cli render $argv --format html" "$directive" ;;
		*) refuse "cannot read the output of: fsa1-cli render $argv --format html" "$directive" ;;
	esac
}

page=$(cat "$PAGE"; printf X)
page=${page%X}

rendered=
rest=$page
frame=0
n=0
declare -a olds=() news=() dirs=() outframe=() outargv=() outdir=()
declare -a framecmds=()

while :; do
	pre=${rest%%<!--fsa1:*}
	if [ "$pre" = "$rest" ]; then rendered=$rendered$rest; break; fi
	rest=${rest#*<!--fsa1:}
	directive=${rest%%-->*}
	if [ "$directive" = "$rest" ]; then refuse 'a directive without its `-->`' "${directive%%$NL*}"; fi
	rest=${rest#*-->}
	body=${rest%%<!--fsa1:end-->*}
	if [ "$body" = "$rest" ]; then refuse 'unterminated region' "$directive"; fi
	case $body in *'<!--fsa1:'*) refuse 'unterminated region' "$directive" ;; esac
	rest=${rest#*<!--fsa1:end-->}

	scan=$pre
	while [ "${scan#*class=\"frame}" != "$scan" ]; do
		scan=${scan#*class=\"frame}
		frame=$((frame + 1))
	done

	verb=${directive%% *}
	if [ "$verb" = "$directive" ]; then args=; else args=${directive#* }; fi
	payload=
	case $verb in
		out|cmd)
			fx=0
			d=0
			if [ "$verb" = out ]; then
				while :; do
					case $args in
						'+fx '*) fx=1; args=${args#+fx } ;;
						'+d '*) d=1; args=${args#+d } ;;
						*) break ;;
					esac
				done
			fi
			if [ -z "$args" ]; then refuse 'a verb without argv' "$directive"; fi
			if [ "$verb" = cmd ]; then
				framecmds[$frame]="${framecmds[$frame]:-}$NL$args"
				payload='<span class="p">$</span> '$(esc "$args")
			else
				status=0
				# Accepted blind spot: `$( )` strips trailing newlines, so a CLI change that adds
				# or drops a final blank line renders identically and passes --check.
				stdout=$(run_cmd "$args") || status=$?
				if [ $status -ne 0 ]; then refuse "exited $status: $args" "$directive"; fi
				payload=$(decorate "$(esc "$stdout")" "$fx" "$d")
				outframe[$n]=$frame
				outargv[$n]=$args
				outdir[$n]=$directive
			fi
			;;
		sheet|style)
			if [ -z "$args" ]; then refuse 'a verb without argv' "$directive"; fi
			anchored "$verb" '' "$args" "$directive"
			;;
		spec)
			name=${args%% -- *}
			if [ "$name" = "$args" ]; then refuse 'a spec region without `<name> -- <argv>`' "$directive"; fi
			anchored spec "$name" "${args#* -- }" "$directive"
			;;
		*) refuse "unknown verb: $verb" "$directive" ;;
	esac

	olds[$n]=$body
	news[$n]=$payload
	dirs[$n]=$directive
	rendered=$rendered$pre'<!--fsa1:'$directive'-->'$payload'<!--fsa1:end-->'
	n=$((n + 1))
done

# `cmd` never runs what it spells, so a frame whose prompt line and output disagree is the one drift
# nothing else here can see. Accepted blind spot: this asks each `out` whether some `cmd` spells it,
# never the reverse, so a `cmd` alone in its frame is checked against nothing — `fsa1-cli unpack
# Q3.xlsx` is the page's one such prompt, and renaming that verb would leave `--check` green.
i=0
while [ $i -lt $n ]; do
	if [ -n "${outdir[$i]:-}" ]; then
		spelled=${framecmds[${outframe[$i]}]:-}
		if [ -n "$spelled" ]; then
			case "$spelled$NL" in
				*"$NL${outargv[$i]}$NL"*) ;;
				*) refuse "no cmd region in this .frame spells: ${outargv[$i]}" "${outdir[$i]}" ;;
			esac
		fi
	fi
	i=$((i + 1))
done

printf '%s' "$rendered" >"$WORK/index.html"

if [ $check -eq 0 ]; then
	cat "$WORK/index.html" >"$PAGE"
	printf 'render-site: %s regions rendered\n' "$n" >&2
	exit 0
fi

if diff "$PAGE" "$WORK/index.html" >/dev/null 2>&1; then exit 0; fi
i=0
while [ $i -lt $n ]; do
	if [ "${olds[$i]}" != "${news[$i]}" ]; then
		refuse 'this region is stale; run ./scripts/render-site.sh' "${dirs[$i]}"
	fi
	i=$((i + 1))
done
printf 'render-site: %s differs outside every region\n' "$PAGE" >&2
exit 1

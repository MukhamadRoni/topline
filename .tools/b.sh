# queue a build: b.sh <id> [args]; waits for completion, prints errors
id=$1; shift; P=$HOME/mnt/topline-project/.tools
printf '& "$PSScriptRoot\\..\\..\\build.ps1" %s\r\nexit $LASTEXITCODE\r\n' "$*" > $P/req/$id-build.ps1
for i in $(seq 1 165); do [ -f $P/out/$id-build.done ] && break; sleep 1; done
cat $P/out/$id-build.done 2>/dev/null || echo "(masih berjalan)"

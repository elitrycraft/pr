/\ypokedata_workaround\y/{split($2,a,"[xX]");pokedata_workaround=hexval(a[2])}
/\y_start\y/{split($2,a,"[xX]");start=hexval(a[2])}
function hexval(s,    i,v,c) {
	v=0; for(i=1;i<=length(s);i++){c=substr(s,i,1);v=v*16+index("0123456789abcdef",tolower(c))-1}; return v
}
END {
	print "#include <unistd.h>"
	print "const ssize_t offset_to_pokedata_workaround=" (pokedata_workaround-start) ";"
}

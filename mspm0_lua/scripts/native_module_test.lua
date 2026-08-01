assert(plug, "plug module missing")
assert(plug.ping() == 3507, "plug.ping failed")
print("PLUG_OK", plug.ping())

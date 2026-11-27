class_name StoryVM
extends RefCounted
## Runs a story: the same rules as the Rust game's script.rs.
##
## The story is the document the story editor saves, as JSON
## (res://story/story.json): characters, properties, things, a clock,
## schedules, and scenes made of items. This class flattens the scenes
## into commands and steps through them; whoever shows the story calls
## next() and presents what it returns.

const DAY := 24 * 60
const AWAY := "away"
const SLOTS := ["left", "center", "right"]

var doc: Dictionary = {}
var cmds: Array = []
var labels: Dictionary = {}
var characters: Dictionary = {}   # id -> character
var things: Dictionary = {}       # id -> thing

var pc := 0
var vars: Dictionary = {}
var clock := 0
var events: Dictionary = {}       # name -> true
var place := ""
var holdings: Dictionary = {}     # "who.thing" -> amount, where it differs from the start
var changes: Array = []           # what the last exchange changed: [who, thing, by]
var done := false                 # whether the last give or trade was carried out
var _spoke := false


func load_doc(d: Dictionary) -> void:
	doc = d
	cmds.clear()
	labels.clear()
	characters.clear()
	things.clear()
	for c in doc.get("characters", []):
		# a day is looked up in time order, however the file lists it
		var day: Array = c.get("schedule", []).duplicate()
		day.sort_custom(func(a, b): return int(a["at"]) < int(b["at"]))
		c["schedule"] = day
		characters[c["id"]] = c
	for t in doc.get("things", []):
		things[t["id"]] = t
	for scene in doc.get("scenes", []):
		if not labels.has(scene["label"]):
			labels[scene["label"]] = cmds.size()
		for item in scene.get("items", []):
			var t: String = item.get("t", "")
			if t == "comment":
				continue
			if t == "show" and not SLOTS.has(item.get("slot", "")):
				continue
			cmds.append(item)
	reset()


## A fresh run: every property at its declared start.
func reset() -> void:
	pc = 0
	vars.clear()
	for v in doc.get("vars", []):
		vars[v["id"]] = int(v.get("initial", 0))
	var c = doc.get("clock")
	clock = int(c["start"]) if c is Dictionary else 0
	events.clear()
	place = ""
	holdings.clear()
	changes.clear()
	done = false
	_spoke = false


func has_clock() -> bool:
	return doc.get("clock") is Dictionary


func per_line() -> int:
	var c = doc.get("clock")
	return clampi(int(c.get("per_line", 0)), 0, 120) if c is Dictionary else 0


## Is this place shown as a 3D world?
func is_3d(p: String) -> bool:
	var own = doc.get("locales", {}).get(p, "")
	if own != "":
		return own == "3d"
	return doc.get("render", "") == "3d"


func jump(label: String) -> void:
	pc = int(labels.get(label, cmds.size()))


static func fit(thing: Dictionary, n: int) -> int:
	match thing.get("kind", "item"):
		"info":
			return clampi(n, 0, 1)
		"item":
			return maxi(n, 0)
	return n


## How much of a thing a character has now.
func has(who: String, thing: String) -> int:
	var key := who + "." + thing
	if holdings.has(key):
		return int(holdings[key])
	if not things.has(thing):
		return 0
	var def: Dictionary = things[thing]
	var start := int(def.get("initial", 0))
	if characters.has(who):
		var own: Dictionary = characters[who].get("holds", {})
		if own.has(thing):
			start = int(own[thing])
	return fit(def, start)


func _room(who: String, thing: String) -> int:
	if things.has(thing) and things[thing].get("kind", "") == "info":
		return 1 - has(who, thing)
	return 2147483647


func _alter(who: String, thing: String, by: int) -> int:
	if not things.has(thing) or not characters.has(who):
		return 0
	var before := has(who, thing)
	var after := fit(things[thing], clampi(before + by, -2147483648, 2147483647))
	if after != before:
		holdings[who + "." + thing] = after
		changes.append([who, thing, after - before])
	return after - before


func _counted(thing: String) -> bool:
	return things.has(thing) and things[thing].get("kind", "") != "attribute"


func _exchange(cmd: Dictionary) -> void:
	changes.clear()
	done = false
	match cmd["t"]:
		"give":
			var from: String = cmd["from"]
			var to: String = cmd["to"]
			var thing: String = cmd["thing"]
			if from == to:
				return
			var n := int(cmd.get("n", 1))
			if _counted(thing):
				n = mini(mini(n, has(from, thing)), _room(to, thing))
			n = maxi(n, 0)
			var taken := -_alter(from, thing, -n)
			_alter(to, thing, taken if _counted(thing) else n)
			done = n > 0
		"copy":
			var from: String = cmd["from"]
			var to: String = cmd["to"]
			var thing: String = cmd["thing"]
			if from == to:
				return
			var held := has(from, thing)
			var n := int(cmd.get("n", 1))
			if _counted(thing):
				if held > 0:
					_alter(to, thing, held if n <= 0 else n)
			else:
				_alter(to, thing, held - has(to, thing))
		"grant":
			_alter(cmd["who"], cmd["thing"], int(cmd.get("n", 0)))
		"trade":
			var a: String = cmd["a"]
			var b: String = cmd["b"]
			var t1: String = cmd["thing"]
			var t2: String = cmd["thing2"]
			if a == b or t1 == t2:
				return
			var n1 := maxi(int(cmd.get("n", 0)), 0)
			var n2 := maxi(int(cmd.get("n2", 0)), 0)
			var ok1 := not _counted(t1) or (has(a, t1) >= n1 and _room(b, t1) >= n1)
			var ok2 := not _counted(t2) or (has(b, t2) >= n2 and _room(a, t2) >= n2)
			if ok1 and ok2:
				done = true
				_alter(a, t1, -n1)
				_alter(b, t1, n1)
				_alter(b, t2, -n2)
				_alter(a, t2, n2)


## A property, or one of the special names: clock, day, event.<name>,
## @<who> (here), @<who>.<place>, <who>.<thing>.
func value(name: String) -> int:
	if vars.has(name):
		return int(vars[name])
	if name == "clock":
		return posmod(clock, DAY)
	if name == "day":
		return int(floor(float(clock) / DAY)) + 1
	if name.begins_with("event."):
		return 1 if events.has(name.substr(6)) else 0
	if name.begins_with("@"):
		var rest := name.substr(1)
		var who := rest
		var where := place
		var dot := rest.find(".")
		if dot >= 0:
			who = rest.substr(0, dot)
			where = rest.substr(dot + 1)
		return 1 if where != "" and where_is(who) == where else 0
	var dot2 := name.find(".")
	if dot2 > 0:
		return has(name.substr(0, dot2), name.substr(dot2 + 1))
	return 0


func test(c: Dictionary) -> bool:
	var v := value(c["var"])
	var n := int(c["val"])
	match c["op"]:
		"==":
			return v == n
		"!=":
			return v != n
		">=":
			return v >= n
		"<=":
			return v <= n
		">":
			return v > n
	return v < n


## The place a character is in now ("" if away, or if they keep no schedule).
func where_is(who: String) -> String:
	if not characters.has(who):
		return ""
	var c: Dictionary = characters[who]
	var found := ""
	var interrupted := false
	for i in c.get("interrupts", []):
		var w: Dictionary = i["when"]
		if not String(w["var"]).begins_with("@") and test(w):
			found = i["place"]
			interrupted = true
			break
	if not interrupted:
		var day: Array = c.get("schedule", [])
		if day.is_empty():
			return ""
		var now := posmod(clock, DAY)
		# the latest entry at or before now; before the day's first, last night's
		var stop: Dictionary = day[day.size() - 1]
		for k in range(day.size() - 1, -1, -1):
			if int(day[k]["at"]) <= now:
				stop = day[k]
				break
		found = stop["place"]
	return "" if found == AWAY else found


## Everyone whose schedule (or an interrupt) has them in this place now, in cast order.
func present(p: String) -> Array:
	var out: Array = []
	if p == "":
		return out
	for c in doc.get("characters", []):
		if where_is(c["id"]) == p:
			out.append(c["id"])
	return out


func has_schedules() -> bool:
	for c in doc.get("characters", []):
		if not c.get("schedule", []).is_empty() or not c.get("interrupts", []).is_empty():
			return true
	return false


## Run control flow and return the next command the presentation must
## handle, as [index, command]; null at the end of the story.
func next():
	# talking takes time: the line just spoken is counted as the story moves on
	if _spoke:
		_spoke = false
		clock += per_line()
	for _guard in range(100000):
		var i := pc
		if i >= cmds.size():
			return null
		var cmd: Dictionary = cmds[i]
		pc += 1
		match cmd["t"]:
			"set":
				vars[cmd["var"]] = int(cmd["n"])
			"add":
				vars[cmd["var"]] = int(vars.get(cmd["var"], 0)) + int(cmd["n"])
				return [i, cmd]
			"time":
				clock += posmod(posmod(int(cmd["at"]), DAY) - posmod(clock, DAY), DAY)
			"pass":
				clock += clampi(int(cmd["minutes"]), 0, 7 * DAY)
			"event":
				if cmd.get("on", true):
					events[cmd["name"]] = true
				else:
					events.erase(cmd["name"])
			"give", "copy", "grant", "trade":
				_exchange(cmd)
				return [i, cmd]
			"bg":
				place = cmd["name"]
				return [i, cmd]
			"say":
				_spoke = true
				return [i, cmd]
			"if":
				if test(cmd):
					jump(cmd["target"])
			"goto":
				jump(cmd["target"])
			_:
				return [i, cmd]
	return null


## The options of a choice that may be taken now.
func open_options(choice: Dictionary) -> Array:
	var out: Array = []
	for o in choice.get("options", []):
		var c = o.get("cond")
		if c == null or test(c):
			out.append(o)
	return out


static func time_text(minutes: int) -> String:
	var m := posmod(minutes, DAY)
	return "%02d:%02d" % [m / 60, m % 60]


## The name of a recorded line's file (without its ending), as the Rust
## game and the editor's Sound view name it: FNV-1a over speaker and text.
static func voice_stem(who: String, text: String) -> String:
	var h := -3750763034362895579   # 0xcbf29ce484222325
	var bytes := (who + "\n" + text.strip_edges()).to_utf8_buffer()
	for b in bytes:
		h = (h ^ int(b)) * 1099511628211
	return "voice_%s_%012x" % [who, h & 0xFFFFFFFFFFFF]

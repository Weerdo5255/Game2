extends Node
## Shows the story: over paintings (Stage2D) or in a 3D place (World3D),
## with the talk, choices, meters and clock in the HUD.
##
## The story itself is run by StoryVM; this script presents each command
## it hands back, and keeps the cast on stage (the scene's own shows and
## hides, and whoever a schedule has in the place).

const STORY := "res://story/story.json"
const NARRATOR := "n"

# (left untyped: each carries its own script, whose methods are called below)
@onready var stage = $Stage2D
@onready var world = $World3D
@onready var hud = $HUD
@onready var music: AudioStreamPlayer = $Music
@onready var voice: AudioStreamPlayer = $Voice

var vm := StoryVM.new()
var mode := "title"           # title, run, say, choice, card, ending
var bg := ""
var slots: Array = [null, null, null]   # each: {who, expr, auto}
var dismissed: Dictionary = {}
var unseated: Dictionary = {}
var presence_place := ""
var current_music := ""
var moves: Array = []         # scheduled comings and goings, as the Rust game logs them
var exchanges: Array = []     # what changed hands
var ending_title := ""
var _open_options: Array = []


func _ready() -> void:
	var text := FileAccess.get_file_as_string(STORY)
	var parsed = JSON.parse_string(text)
	if not parsed is Dictionary:
		push_error("res://story/story.json is missing or unreadable: export the story first (cargo run -- --export-godot godot)")
		parsed = {"title": "No story", "scenes": []}
	vm.load_doc(parsed)
	hud.setup(vm)
	hud.advance.connect(_on_advance)
	hud.chosen.connect(_on_chosen)
	hud.start_pressed.connect(new_game)
	_show_title()


func _show_title() -> void:
	mode = "title"
	_clear_scene()
	stage.show_background("vista_night")
	hud.show_title(vm.doc.get("title", ""), vm.doc.get("subtitle", ""))


func new_game(label := "start") -> void:
	vm.reset()
	moves.clear()
	exchanges.clear()
	ending_title = ""
	_clear_scene()
	hud.hide_overlays()
	vm.jump(label)
	mode = "run"
	run()


func _clear_scene() -> void:
	bg = ""
	vm.place = ""
	slots = [null, null, null]
	dismissed.clear()
	unseated.clear()
	presence_place = ""
	voice.stop()
	_play_music("")
	hud.clear_toasts()
	_present_place()
	_present_cast()


## Step the story until it needs the player: a line, a choice, a card or an ending.
func run() -> void:
	while true:
		var step = vm.next()
		if step == null:
			_show_title()
			return
		var cmd: Dictionary = step[1]
		match cmd["t"]:
			"bg":
				if cmd["name"] != bg:
					bg = cmd["name"]
					slots = [null, null, null]
					dismissed.clear()
					presence_place = ""
					_present_place()
					sync_presence()
			"show":
				var slot: int = StoryVM.SLOTS.find(cmd["slot"])
				var who: String = cmd["who"]
				# someone here by their schedule makes room: to a free place if there is one
				var old = slots[slot]
				if old != null and old["auto"] and old["who"] != who:
					var free := -1
					for k in range(3):
						if k != slot and slots[k] == null:
							free = k
							break
					if free >= 0:
						slots[free] = old
					else:
						unseated[old["who"]] = true
				slots[slot] = {"who": who, "expr": cmd["expr"], "auto": false}
				for k in range(3):
					if k != slot and slots[k] != null and slots[k]["who"] == who:
						slots[k] = null
				dismissed.erase(who)
				_present_cast()
			"hide":
				var which: int = StoryVM.SLOTS.find(cmd["slot"])   # -1: everyone
				for k in range(3):
					if (which < 0 or which == k) and slots[k] != null:
						dismissed[slots[k]["who"]] = true
						slots[k] = null
				_present_cast()
			"music":
				_play_music(cmd["track"])
			"add":
				_announce_change(cmd)
			"give", "copy", "grant", "trade":
				_announce_exchange(cmd)
			"say":
				sync_presence()
				mode = "say"
				_say(cmd["who"], cmd["text"])
				return
			"choice":
				sync_presence()
				mode = "choice"
				_open_options = vm.open_options(cmd)
				var texts: Array = []
				for o in _open_options:
					texts.append(o["text"])
				hud.show_choice(texts)
				return
			"card":
				_clear_scene()
				mode = "card"
				hud.show_card(cmd.get("title", ""), cmd.get("sub", ""))
				return
			"end":
				mode = "ending"
				ending_title = cmd.get("title", "")
				hud.clear_toasts()
				hud.show_ending(ending_title, cmd.get("text", ""))
				return


func _on_advance() -> void:
	match mode:
		"say":
			if hud.finish_line():
				return   # the line was still being written out: now it is all there
			hud.hide_line()
			mode = "run"
			run()
		"card":
			hud.hide_overlays()
			mode = "run"
			run()
		"ending":
			_show_title()


func _on_chosen(index: int) -> void:
	if mode != "choice" or index < 0 or index >= _open_options.size():
		return
	hud.hide_choice()
	vm.jump(_open_options[index]["target"])
	mode = "run"
	run()


## For tests: take a choice by its place in the list (the last one if out of range).
func pick(index: int) -> void:
	if mode == "choice" and not _open_options.is_empty():
		_on_chosen(mini(index, _open_options.size() - 1))


func _name_of(who: String) -> String:
	return vm.characters[who].get("name", who) if vm.characters.has(who) else who


func _colour_of(who: String) -> Color:
	if who != NARRATOR and vm.characters.has(who):
		var c: String = vm.characters[who].get("color", "")
		if Color.html_is_valid(c) and c != "":
			return Color.html(c)
	return Color.WHITE


func _say(who: String, text: String) -> void:
	var speaking := who if _on_stage(who) else ""
	for k in range(3):
		if slots[k] != null:
			slots[k]["dim"] = speaking != "" and slots[k]["who"] != speaking
	_present_cast()
	var label := "" if who == NARRATOR or not vm.characters.has(who) else _name_of(who)
	hud.show_line(label, _colour_of(who), text, who == NARRATOR)
	_play_voice(who, text)


func _on_stage(who: String) -> bool:
	for s in slots:
		if s != null and s["who"] == who:
			return true
	return false


func _has_portrait(who: String) -> bool:
	return vm.characters.has(who) and vm.characters[who].get("sprite", "") != ""


## Bring the cast on stage into line with the characters' schedules.
func sync_presence() -> void:
	if not vm.has_schedules():
		return
	var quiet := presence_place != bg
	presence_place = bg
	var stamp := StoryVM.time_text(vm.clock)
	var here: Array = []
	for who in vm.present(bg):
		if _has_portrait(who):
			here.append(who)
	for k in range(3):
		var s = slots[k]
		if s != null and s["auto"] and not here.has(s["who"]):
			slots[k] = null
			moves.append("%s %s leaves %s" % [stamp, s["who"], bg])
			if not quiet:
				hud.toast("%s leaves" % _name_of(s["who"]), _colour_of(s["who"]))
	for who in dismissed.keys():
		if not here.has(who):
			dismissed.erase(who)
	for who in unseated.keys():
		if not here.has(who):
			unseated.erase(who)
	for who in here:
		if dismissed.has(who) or _on_stage(who):
			continue
		# the sides first, the middle last; with no room they are here all the same, out of view
		var free := -1
		for k in [0, 2, 1]:
			if slots[k] == null:
				free = k
				break
		if free < 0:
			unseated[who] = true
			continue
		var silently: bool = quiet or unseated.has(who)
		unseated.erase(who)
		slots[free] = {"who": who, "expr": "neutral", "auto": true}
		moves.append("%s %s %s %s" % [stamp, who, "is in" if silently else "arrives in", bg])
		if not silently:
			hud.toast("%s arrives" % _name_of(who), _colour_of(who))
	_present_cast()


func _announce_change(cmd: Dictionary) -> void:
	for v in vm.doc.get("vars", []):
		if v["id"] == cmd["var"]:
			if v.get("hidden", false) or v.get("label", "") == "":
				return
			var n := int(cmd["n"])
			var good: bool = (n > 0) == bool(v.get("good", true))
			hud.toast("%s %s%d" % [v["label"], "+" if n > 0 else "", n], Color(0.59, 0.94, 0.63) if good else Color(1.0, 0.51, 0.47))
			hud.refresh_meters()
			return


func _announce_exchange(cmd: Dictionary) -> void:
	var shown := func(thing: String) -> bool:
		return vm.things.has(thing) and not vm.things[thing].get("hidden", false)
	if not vm.done and cmd["t"] == "trade":
		if shown.call(cmd["thing"]) and shown.call(cmd["thing2"]):
			hud.toast("%s and %s: no trade" % [_name_of(cmd["a"]), _name_of(cmd["b"])], Color(0.78, 0.78, 0.84))
	for ch in vm.changes:
		var who: String = ch[0]
		var thing: String = ch[1]
		var by: int = ch[2]
		exchanges.append("%s %s %s%d" % [who, thing, "+" if by > 0 else "", by])
		if not shown.call(thing):
			continue
		var def: Dictionary = vm.things[thing]
		var label: String = def.get("label", "") if def.get("label", "") != "" else thing
		var text := ""
		if def.get("kind", "") == "info":
			text = "%s %s: %s" % [_name_of(who), "learns" if by > 0 else "forgets", label]
		else:
			text = "%s: %s %s%d" % [_name_of(who), label, "+" if by > 0 else "", by]
		hud.toast(text, Color(0.59, 0.94, 0.63) if by > 0 else Color(1.0, 0.75, 0.47))
	hud.refresh_ledger()


# ---- how it is shown

func _in_world() -> bool:
	return bg != "" and vm.is_3d(bg) and world.has_place(bg)


func _present_place() -> void:
	if _in_world():
		stage.visible = false
		world.enter(bg)
	else:
		world.leave()
		stage.visible = true
		stage.show_background(bg)
	hud.set_clock_visible(vm.has_clock() and bg != "")


func _present_cast() -> void:
	var cast: Array = []
	for s in slots:
		if s == null or not _has_portrait(s["who"]):
			cast.append(null)
		else:
			cast.append({"who": s["who"], "expr": s["expr"], "dim": s.get("dim", false), "def": vm.characters[s["who"]]})
	if _in_world():
		world.show_cast(cast)
	else:
		stage.show_cast(cast)


func _find_audio(stem: String) -> String:
	for ext in ["ogg", "mp3", "wav"]:
		var path := "res://audio/%s.%s" % [stem, ext]
		if ResourceLoader.exists(path):
			return path
	return ""


func _play_music(track: String) -> void:
	if track == current_music:
		return
	current_music = track
	music.stop()
	if track == "":
		return
	var path := _find_audio("music_" + track)
	if path == "":
		return
	var stream = load(path)
	# music goes round until another track starts
	if stream is AudioStreamOggVorbis or stream is AudioStreamMP3:
		stream.loop = true
	elif stream is AudioStreamWAV:
		stream.loop_mode = AudioStreamWAV.LOOP_FORWARD
		stream.loop_end = int(stream.get_length() * stream.mix_rate)
	music.stream = stream
	music.play()


func _play_voice(who: String, text: String) -> void:
	voice.stop()
	var path := _find_audio(StoryVM.voice_stem(who, text))
	if path != "":
		voice.stream = load(path)
		voice.play()


func _process(_delta: float) -> void:
	if mode == "say" or mode == "choice":
		hud.set_clock(vm.clock)


func _unhandled_input(event: InputEvent) -> void:
	if event.is_action_pressed("ui_cancel"):
		if mode == "title":
			get_tree().quit()
		else:
			_show_title()
	elif event is InputEventKey and event.pressed and not event.echo:
		if event.keycode == KEY_I:
			hud.toggle_ledger()
		elif event.keycode == KEY_M:
			var bus := AudioServer.get_bus_index("Master")
			AudioServer.set_bus_mute(bus, not AudioServer.is_bus_mute(bus))

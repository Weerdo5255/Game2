extends CanvasLayer
## What lies over the story's world: the talk, the choices, the meters,
## the clock, announcements, who has what, chapter cards, endings and the
## title. The layout is in hud.tscn and is yours to restyle in Godot.

signal advance
signal chosen(index: int)
signal start_pressed

@onready var clock_label: Label = $Clock
@onready var meters: VBoxContainer = $Meters
@onready var toasts: VBoxContainer = $Toasts
@onready var dialogue: PanelContainer = $Dialogue
@onready var name_label: Label = $Dialogue/Margin/Lines/Name
@onready var text_label: Label = $Dialogue/Margin/Lines/Text
@onready var choices: PanelContainer = $Choices
@onready var choice_list: VBoxContainer = $Choices/Margin/List
@onready var ledger: PanelContainer = $Ledger
@onready var ledger_text: Label = $Ledger/Margin/Text
@onready var card: ColorRect = $Card
@onready var ending: ColorRect = $Ending
@onready var title: Control = $Title

var vm: StoryVM
var _shown := 0.0          # characters of the line written out so far
var _writing := false


func setup(story: StoryVM) -> void:
	vm = story
	$Title/Box/Start.pressed.connect(func(): start_pressed.emit())
	$Title/Box/Quit.pressed.connect(func(): get_tree().quit())
	hide_overlays()
	hide_line()
	hide_choice()
	ledger.visible = false
	clock_label.visible = false
	refresh_meters()


func _process(delta: float) -> void:
	if _writing:
		_shown += delta * 48.0
		text_label.visible_characters = int(_shown)
		if int(_shown) >= text_label.text.length():
			text_label.visible_characters = -1
			_writing = false


func _unhandled_input(event: InputEvent) -> void:
	if title.visible or choices.visible:
		return
	var click: bool = event is InputEventMouseButton and event.pressed and event.button_index == MOUSE_BUTTON_LEFT
	if click or event.is_action_pressed("ui_accept"):
		advance.emit()


# ---- the talk

func show_line(speaker: String, colour: Color, text: String, narration: bool) -> void:
	name_label.text = speaker
	name_label.visible = speaker != ""
	name_label.add_theme_color_override("font_color", colour)
	text_label.text = text
	text_label.add_theme_color_override("font_color", Color(0.75, 0.82, 1.0) if narration else Color.WHITE)
	text_label.visible_characters = 0
	_shown = 0.0
	_writing = true
	dialogue.visible = true
	refresh_meters()
	refresh_ledger()


## If the line is still being written out, show all of it and say so.
func finish_line() -> bool:
	if _writing:
		_writing = false
		text_label.visible_characters = -1
		return true
	return false


func hide_line() -> void:
	dialogue.visible = false
	_writing = false


# ---- choices

func show_choice(texts: Array) -> void:
	for child in choice_list.get_children():
		child.queue_free()
	for i in range(texts.size()):
		var button := Button.new()
		button.text = texts[i]
		button.alignment = HORIZONTAL_ALIGNMENT_LEFT
		button.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
		button.pressed.connect(func(): chosen.emit(i))
		choice_list.add_child(button)
		if i == 0:
			button.grab_focus.call_deferred()
	dialogue.visible = false
	choices.visible = true
	refresh_meters()
	refresh_ledger()


func hide_choice() -> void:
	choices.visible = false


# ---- meters, clock, announcements, who has what

func refresh_meters() -> void:
	if vm == null:
		return
	for child in meters.get_children():
		child.queue_free()
	var shown := 0
	for v in vm.doc.get("vars", []):
		var pips := int(v.get("meter", 0))
		if pips <= 0 or shown >= 4:
			continue
		shown += 1
		var row := HBoxContainer.new()
		row.alignment = BoxContainer.ALIGNMENT_END
		var label := Label.new()
		label.text = String(v.get("label", v["id"])).to_upper()
		row.add_child(label)
		var have := vm.value(v["id"])
		var colour := Color(0.47, 0.9, 0.59) if v.get("good", true) else Color(1.0, 0.43, 0.39)
		for k in range(mini(pips, 12)):
			var pip := ColorRect.new()
			pip.custom_minimum_size = Vector2(15, 14)
			pip.color = colour if k < have else Color(0.24, 0.25, 0.39)
			row.add_child(pip)
		meters.add_child(row)


func set_clock_visible(on: bool) -> void:
	clock_label.visible = on


func set_clock(minutes: int) -> void:
	clock_label.text = "DAY %d  %s" % [int(floor(float(minutes) / StoryVM.DAY)) + 1, StoryVM.time_text(minutes)]


func toast(text: String, colour: Color) -> void:
	var label := Label.new()
	label.text = text
	label.add_theme_color_override("font_color", colour)
	toasts.add_child(label)
	while toasts.get_child_count() > 8:
		toasts.get_child(0).free()
	get_tree().create_timer(3.5).timeout.connect(func():
		if is_instance_valid(label):
			label.queue_free())


func clear_toasts() -> void:
	for child in toasts.get_children():
		child.queue_free()


func toggle_ledger() -> void:
	if vm == null or vm.things.is_empty():
		return
	ledger.visible = not ledger.visible
	refresh_ledger()


func refresh_ledger() -> void:
	if vm == null or not ledger.visible:
		return
	var lines: Array = []
	for c in vm.doc.get("characters", []):
		var holds: Array = []
		for t in vm.doc.get("things", []):
			if t.get("hidden", false):
				continue
			var n := vm.has(c["id"], t["id"])
			var label: String = t.get("label", "") if t.get("label", "") != "" else t["id"]
			if t.get("kind", "") == "attribute":
				holds.append("%s %d" % [label, n])
			elif n == 0:
				continue
			elif t.get("kind", "") == "info":
				holds.append("knows: " + label)
			elif n == 1:
				holds.append(label)
			else:
				holds.append("%s %d" % [label, n])
		if not holds.is_empty():
			lines.append("%s:  %s" % [c.get("name", c["id"]), ", ".join(holds)])
	ledger_text.text = "WHO HAS WHAT\n\n" + ("\n".join(lines) if not lines.is_empty() else "Nobody has anything of note.")


# ---- cards, endings, the title

func hide_overlays() -> void:
	card.visible = false
	ending.visible = false
	title.visible = false


func show_card(heading: String, sub: String) -> void:
	hide_line()
	hide_choice()
	$Card/Box/Title.text = heading
	$Card/Box/Sub.text = sub
	card.visible = true


func show_ending(heading: String, text: String) -> void:
	hide_line()
	hide_choice()
	$Ending/Box/Title.text = heading
	$Ending/Box/Text.text = text
	ending.visible = true


func show_title(heading: String, sub: String) -> void:
	hide_overlays()
	hide_line()
	hide_choice()
	ledger.visible = false
	clock_label.visible = false
	$Title/Box/Name.text = heading if heading != "" else "Untitled Story"
	$Title/Box/Sub.text = sub
	title.visible = true
	$Title/Box/Start.grab_focus.call_deferred()

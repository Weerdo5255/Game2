extends Node3D
## The story in a 3D place: one of the scenes in res://places, the
## player walking in it, and the cast standing at its markers.
##
## A place scene is an ordinary Godot scene, yours to edit. What this
## script looks for in it, by name:
##   Spawn                         Marker3D: where the player starts, and which way they face
##   CastLeft, CastCentre, CastRight   Marker3D: where the cast stands
##   anything with collision       StaticBody3D shapes are what the player walks around

const CHARACTER := preload("res://scenes/character_3d.tscn")
const PLAYER := preload("res://scenes/player.tscn")

var place_name := ""
var place = null    # the place's scene, once it is in the tree
var player = null   # scenes/player.tscn
var figures: Array = [null, null, null]


func has_place(p: String) -> bool:
	return ResourceLoader.exists("res://places/%s.tscn" % p)


func enter(p: String) -> void:
	if p == place_name and place != null:
		return
	leave()
	place_name = p
	var scene = load("res://places/%s.tscn" % p)
	if scene == null:
		push_error("res://places/%s.tscn could not be loaded" % p)
		place_name = ""
		return
	place = scene.instantiate()
	add_child(place)
	player = PLAYER.instantiate()
	add_child(player)
	var spawn := place.get_node_or_null("Spawn") as Node3D
	if spawn != null:
		player.global_transform = spawn.global_transform
	player.make_current()
	visible = true


func leave() -> void:
	for k in range(3):
		if figures[k] != null:
			figures[k].queue_free()
			figures[k] = null
	if player != null:
		player.queue_free()
		player = null
	if place != null:
		place.queue_free()
		place = null
	place_name = ""
	visible = false


func show_cast(cast: Array) -> void:
	if place == null:
		return
	var markers := ["CastLeft", "CastCentre", "CastRight"]
	for k in range(3):
		var c = cast[k]
		if c == null:
			if figures[k] != null:
				figures[k].queue_free()
				figures[k] = null
			continue
		if figures[k] == null:
			figures[k] = CHARACTER.instantiate()
			add_child(figures[k])
		var marker := place.get_node_or_null(markers[k]) as Node3D
		if marker != null:
			figures[k].global_position = marker.global_position
		figures[k].floating = bool(place.get("floating")) if place.get("floating") != null else false
		figures[k].present(c["def"], c["expr"], c["dim"])


func _process(_delta: float) -> void:
	# the cast turn to face whoever is looking at them
	if player == null:
		return
	for f in figures:
		if f != null:
			f.face(player.global_position)

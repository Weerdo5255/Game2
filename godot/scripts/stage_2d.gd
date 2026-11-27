extends Control
## The story over paintings: a background and up to three portraits.
## The pictures are res://art/bg/<place>.png and
## res://art/portraits/<who>_<expression>.png (replace any of them with
## your own; the names are what matters).

@onready var background: TextureRect = $Background
@onready var portraits: Array = [$Left, $Centre, $Right]


func show_background(place: String) -> void:
	var path := "res://art/bg/%s.png" % place
	background.texture = load(path) if place != "" and ResourceLoader.exists(path) else null


func show_cast(cast: Array) -> void:
	for k in range(3):
		var rect: TextureRect = portraits[k]
		var c = cast[k]
		if c == null:
			rect.visible = false
			continue
		var path := "res://art/portraits/%s_%s.png" % [c["who"], c["expr"]]
		if not ResourceLoader.exists(path):
			path = "res://art/portraits/%s_neutral.png" % c["who"]
		rect.texture = load(path) if ResourceLoader.exists(path) else null
		rect.modulate = Color(0.55, 0.55, 0.6) if c["dim"] else Color.WHITE
		rect.visible = true

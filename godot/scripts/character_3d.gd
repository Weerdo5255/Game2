extends Node3D
## One of the cast in a 3D place. How they appear is the character's own
## choice in the story: a cut-out of their portrait, a figure built from
## their look, or a model file from res://models.

const CUTOUT_HEIGHT := 1.6
const CUTOUT_LIFT := 0.72

var floating := false
var _shown := ""       # what is built now: who + how + expression
var _body: Node3D = null


func present(def: Dictionary, expr: String, dim: bool) -> void:
	var how: String = def.get("model", "")
	if how != "" and how != "figure" and not ResourceLoader.exists("res://models/" + how):
		how = ""   # the model file is not there: a cut-out, as ever
	var key := "%s|%s|%s" % [def["id"], how, expr if how == "" or how == "figure" else ""]
	if key != _shown:
		_shown = key
		if _body != null:
			_body.queue_free()
		if how == "":
			_body = _cutout(def["id"], expr)
		elif how == "figure":
			_body = _figure(def, expr)
		else:
			_body = load("res://models/" + how).instantiate()
			var s := float(def.get("model_scale", 0.0))
			_body.scale = Vector3.ONE * (s if s > 0.0 else 1.0)
		add_child(_body)
	if _body is Sprite3D:
		_body.modulate = Color(0.55, 0.55, 0.6) if dim else Color.WHITE


func face(point: Vector3) -> void:
	var d := point - global_position
	rotation.y = atan2(d.x, d.z)
	if floating:
		position.y += sin(Time.get_ticks_msec() / 1000.0 * 1.3 + position.x) * 0.0015


func _cutout(who: String, expr: String) -> Sprite3D:
	var sprite := Sprite3D.new()
	var path := "res://art/portraits/%s_%s.png" % [who, expr]
	if not ResourceLoader.exists(path):
		path = "res://art/portraits/%s_neutral.png" % who
	if ResourceLoader.exists(path):
		sprite.texture = load(path)
		sprite.pixel_size = CUTOUT_HEIGHT / float(sprite.texture.get_height())
	sprite.position.y = CUTOUT_LIFT + CUTOUT_HEIGHT / 2.0
	sprite.shaded = false
	sprite.double_sided = true
	sprite.texture_filter = BaseMaterial3D.TEXTURE_FILTER_LINEAR
	return sprite


static func _tone(look: Dictionary, key: String, fallback: String) -> Color:
	var v: String = str(look.get(key, ""))
	return Color.html(v) if v.begins_with("#") and v.length() == 7 and Color.html_is_valid(v) else Color.html(fallback)


## A figure from simple shapes, coloured from the character's look, with
## a face that follows the expression. It stands at the origin facing +z.
func _figure(def: Dictionary, expr: String) -> Node3D:
	var root := Node3D.new()
	var look: Dictionary = def.get("look_all", def.get("look", {}))
	var skin := _tone(look, "skin", "#d8a880")
	var hair := _tone(look, "hair", "#4a3828")
	var coat := _tone(look, "coat", "#3c4660")
	var iris := _tone(look, "iris", "#3a2a20")
	var lips := _tone(look, "lips", "#a85a50")
	var dark := Color(coat.r * 0.55, coat.g * 0.55, coat.b * 0.55)
	var put := func(mesh: Mesh, at: Vector3, size: Vector3, tilt: float, colour: Color, rough: float) -> void:
		var part := MeshInstance3D.new()
		part.mesh = mesh
		var material := StandardMaterial3D.new()
		material.albedo_color = colour
		material.roughness = rough
		part.material_override = material
		part.position = at
		part.rotation.z = tilt
		part.scale = size
		root.add_child(part)
	var cube := BoxMesh.new()
	var drum := CylinderMesh.new()
	drum.top_radius = 0.5
	drum.bottom_radius = 0.5
	drum.height = 1.0
	var ball := SphereMesh.new()
	ball.radius = 0.5
	ball.height = 1.0
	for side in [-1.0, 1.0]:
		put.call(drum, Vector3(side * 0.1, 0.44, 0.0), Vector3(0.17, 0.82, 0.17), 0.0, dark, 0.9)
		put.call(cube, Vector3(side * 0.1, 0.04, 0.05), Vector3(0.14, 0.08, 0.28), 0.0, Color.html("#1c1a1c"), 0.5)
		put.call(drum, Vector3(side * 0.29, 1.13, 0.0), Vector3(0.12, 0.6, 0.12), side * 0.09, coat, 0.8)
		put.call(ball, Vector3(side * 0.32, 0.8, 0.0), Vector3(0.1, 0.12, 0.1), 0.0, skin, 0.7)
		put.call(ball, Vector3(side * 0.2, 1.4, 0.0), Vector3(0.17, 0.14, 0.17), 0.0, coat, 0.8)
	put.call(cube, Vector3(0.0, 1.13, 0.0), Vector3(0.42, 0.62, 0.24), 0.0, coat, 0.8)
	put.call(cube, Vector3(0.0, 1.2, 0.123), Vector3(0.1, 0.4, 0.01), 0.0, Color.html("#f2efe8"), 0.6)
	put.call(drum, Vector3(0.0, 1.47, 0.0), Vector3(0.11, 0.08, 0.11), 0.0, skin, 0.7)
	put.call(ball, Vector3(0.0, 1.62, 0.0), Vector3(0.25, 0.29, 0.26), 0.0, skin, 0.7)
	put.call(ball, Vector3(0.0, 1.665, -0.025), Vector3(0.275, 0.25, 0.27), 0.0, hair, 0.9)
	put.call(ball, Vector3(0.0, 1.6, -0.06), Vector3(0.26, 0.26, 0.2), 0.0, hair, 0.9)
	put.call(ball, Vector3(0.0, 1.6, 0.128), Vector3(0.03, 0.045, 0.03), 0.0, skin, 0.7)
	# brow tilt, brow lift, eye opening, mouth width, mouth height, mouth drop
	var face := [0.0, 0.0, 1.0, 0.06, 0.012, 0.0]
	match expr:
		"smile":
			face = [0.0, 0.0, 0.8, 0.085, 0.014, 0.0]
		"worried":
			face = [-0.35, 0.006, 1.0, 0.05, 0.012, -0.004]
		"angry":
			face = [0.4, -0.008, 0.8, 0.06, 0.012, -0.004]
		"smug":
			face = [0.15, 0.002, 0.7, 0.07, 0.012, 0.002]
		"shock":
			face = [0.0, 0.014, 1.35, 0.04, 0.045, -0.008]
	for side in [-1.0, 1.0]:
		put.call(ball, Vector3(side * 0.048, 1.635, 0.112), Vector3(0.036, 0.03 * face[2], 0.02), 0.0, Color.html("#f4f1ec"), 0.3)
		put.call(ball, Vector3(side * 0.048, 1.635, 0.121), Vector3(0.02, 0.022 * face[2], 0.012), 0.0, iris, 0.3)
		put.call(cube, Vector3(side * 0.048, 1.667 + face[1], 0.118), Vector3(0.045, 0.008, 0.01), -side * face[0], hair, 0.9)
	put.call(cube, Vector3(0.0, 1.548 + face[5], 0.118), Vector3(face[3], face[4], 0.012), 0.0, lips, 0.6)
	return root

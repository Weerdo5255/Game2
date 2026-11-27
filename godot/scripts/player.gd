extends CharacterBody3D
## The player in a 3D place: walks on the level, slides round whatever
## has a collision shape.
##   W A S D        walk (Shift to hurry)
##   Left / Right   turn (or Q / E)
##   right mouse    hold and move to look around

@export var walk_speed := 2.6
@export var run_speed := 5.2
@export var turn_speed := 1.9
@export var eye_height := 1.62

@onready var head: Node3D = $Head
@onready var camera: Camera3D = $Head/Camera3D

var pitch := 0.0


func _ready() -> void:
	motion_mode = CharacterBody3D.MOTION_MODE_FLOATING
	head.position.y = eye_height


func make_current() -> void:
	camera.make_current()


func _unhandled_input(event: InputEvent) -> void:
	if event is InputEventMouseMotion and Input.is_mouse_button_pressed(MOUSE_BUTTON_RIGHT):
		rotate_y(-event.relative.x * 0.004)
		pitch = clampf(pitch - event.relative.y * 0.004, -1.2, 1.2)
		head.rotation.x = pitch


func _physics_process(delta: float) -> void:
	var turn := 0.0
	if Input.is_key_pressed(KEY_LEFT) or Input.is_key_pressed(KEY_Q):
		turn += 1.0
	if Input.is_key_pressed(KEY_RIGHT) or Input.is_key_pressed(KEY_E):
		turn -= 1.0
	rotate_y(turn * turn_speed * delta)
	var step := Vector3.ZERO
	if Input.is_key_pressed(KEY_W):
		step -= transform.basis.z
	if Input.is_key_pressed(KEY_S):
		step += transform.basis.z
	if Input.is_key_pressed(KEY_D):
		step += transform.basis.x
	if Input.is_key_pressed(KEY_A):
		step -= transform.basis.x
	step.y = 0.0
	var speed := run_speed if Input.is_key_pressed(KEY_SHIFT) else walk_speed
	velocity = step.normalized() * speed
	move_and_slide()

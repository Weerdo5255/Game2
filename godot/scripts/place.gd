extends Node3D
## A 3D place. The story names it with a Background block; this scene is
## what is shown where the story (or that place) is set to 3D.
##
## Edit it freely in Godot. Keep the markers (Spawn, CastLeft, CastCentre,
## CastRight); give anything the player should walk around a StaticBody3D
## with a collision shape.

## People drift here rather than stand (no gravity).
@export var floating := false

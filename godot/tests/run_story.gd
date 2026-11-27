extends SceneTree
## Plays the exported story with no window and prints how it went, in the
## same words as the Rust game's --autoplay, so the two can be compared:
##
##   godot --headless --path godot --script res://tests/run_story.gd -- 0,0,1,0,0,2,3 [start-label]
##
## The numbers are the choices to take, in order (0 for any not given).

func _initialize() -> void:
	var args := OS.get_cmdline_user_args()
	var picks: Array = []
	if args.size() > 0:
		for p in String(args[0]).split(",", false):
			picks.append(int(p))
	var start := String(args[1]) if args.size() > 1 else "start"
	var game = load("res://main.tscn").instantiate()
	root.add_child(game)
	await process_frame
	game.new_game(start)
	var taken := 0
	for _step in range(100000):
		match game.mode:
			"say", "card":
				game.hud.finish_line()
				game._on_advance()
			"choice":
				game.pick(picks[taken] if taken < picks.size() else 0)
				taken += 1
			"ending":
				break
			_:
				break
	# (like the Rust game: a run that reaches no ending reports nothing on its output)
	if game.mode == "ending":
		if not game.moves.is_empty():
			print("schedule: ", "; ".join(game.moves))
		if not game.exchanges.is_empty():
			print("exchanges: ", "; ".join(game.exchanges))
		print("ending: ", game.ending_title)
	else:
		printerr("stopped without an ending, in mode ", game.mode)
	quit()

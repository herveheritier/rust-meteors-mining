$INCLUDEONCE

'$INCLUDE:'./world_type.bas'
'$INCLUDE:'./player_type.bas'

type context_type
    active_shapes as integer
    free_shapes as integer
    deleted_shapes as integer
    messageDelay as double
    message as string
    messageQueue as string
    message1 as string
    message2 as string
    world as world_type
    fps as integer
    playerAtStation as integer
    playerEnterStation as integer
    meteorsDestroyed as integer
    bulletsFired as integer
    bulletsLost as integer
    player as player_type
    movingMode as integer
    VIEWPORT_WIDTH as integer
    VIEWPORT_HEIGHT as integer
    EXTERNAL_BORDER as integer
    WORLD_WIDTH as integer
    WORLD_HEIGHT as integer
    WORLD_MINX as integer
    WORLD_MAXX as integer
    WORLD_MINY as integer
    WORLD_MAXY as integer
    DRAW_MINX as integer
    DRAW_MAXX as integer
    DRAW_MINY as integer
    DRAW_MAXY as integer
    STARS_COUNT as long
    STARS_LAYERS as integer
    SHAPES_COUNT as integer
    TRIANGLES_IN_SHAPE_MIN as integer
    TRIANGLES_IN_SHAPE_MAX as integer
    TRIANGLE_BASE_MIN as integer
    TRIANGLE_BASE_MAX as integer
    TRIANGLE_HEIGHT_MIN as integer
    TRIANGLE_HEIGHT_MAX as integer
    ATTEMPT_FPS as integer
    FULL_SCREEN as integer
end type

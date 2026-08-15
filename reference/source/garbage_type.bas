$INCLUDEONCE

'$INCLUDE:'./context_type.bas'
'$INCLUDE:'./point_type.bas'
'$INCLUDE:'./triangle_type.bas'
'$INCLUDE:'./shape_type.bas'

type garbage_type
    position as point_type
    radius as double
    direction as double
    velocity as double
    orientation as double
    angle as double
    life as integer
    rgbaColor as _unsigned long
end type

'                                                                 #####
'  ####   ######  #    #  ######  #####     ##    #####  ######  #     #    ##    #####   #####     ##     ####   ######   ####
' #    #  #       ##   #  #       #    #   #  #     #    #       #         #  #   #    #  #    #   #  #   #    #  #       #
' #       #####   # #  #  #####   #    #  #    #    #    #####   #  ####  #    #  #    #  #####   #    #  #       #####    ####
' #  ###  #       #  # #  #       #####   ######    #    #       #     #  ######  #####   #    #  ######  #  ###  #            #
' #    #  #       #   ##  #       #   #   #    #    #    #       #     #  #    #  #   #   #    #  #    #  #    #  #       #    #
'  ####   ######  #    #  ######  #    #  #    #    #    ######   #####   #    #  #    #  #####   #    #   ####   ######   ####

sub generateGarbages (garbages() as garbage_type, t as triangle_type, shapes() as shape_type)
    dim g as garbage_type
    for i% = 1 to 12
        g.position = t.realCenter
        g.radius = rnd * 2
        g.direction = rnd * TAU 'shapes(t.shapeIndex).direction '+ rnd*(TAU/8)
        g.velocity = shapes(t.shapeIndex).velocity * (1 + rnd * 3)
        g.orientation = rnd * TAU
        g.life = rnd * 255 \ 7
        g.rgbaColor = &HFFFFFFFF '_rgba32(rnd*128+127,rnd*128+127,rnd*128+127,255)
        for j% = lbound(garbages) to ubound(garbages)
            if garbages(j%).life = 0 then exit for
        next j%
        if j% > ubound(garbages) then
            redim _preserve garbages(0 to ubound(garbages) + 1) as garbage_type
            garbages(ubound(garbages)) = g
        else
            garbages(j%) = g
        end if
    next i%
end sub

'                                             #####
' #    #   ####   #    #  #  #    #   ####   #     #    ##    #####   #####     ##     ####   ######
' ##  ##  #    #  #    #  #  ##   #  #    #  #         #  #   #    #  #    #   #  #   #    #  #
' # ## #  #    #  #    #  #  # #  #  #       #  ####  #    #  #    #  #####   #    #  #       #####
' #    #  #    #  #    #  #  #  # #  #  ###  #     #  ######  #####   #    #  ######  #  ###  #
' #    #  #    #   #  #   #  #   ##  #    #  #     #  #    #  #   #   #    #  #    #  #    #  #
' #    #   ####     ##    #  #    #   ####    #####   #    #  #    #  #####   #    #   ####   ######

sub movingGarbage (g as garbage_type, fps%)
    if g.life = 0 then exit sub
    g.life = g.life - 1
    g.position.x = g.position.x + cos(g.direction) * 60 * g.velocity / fps%
    g.position.y = g.position.y - sin(g.direction) * 60 * g.velocity / fps%
end sub

'                                  #####
' #####   #####     ##    #    #  #     #    ##    #####   #####     ##     ####   ######
' #    #  #    #   #  #   #    #  #         #  #   #    #  #    #   #  #   #    #  #
' #    #  #    #  #    #  #    #  #  ####  #    #  #    #  #####   #    #  #       #####
' #    #  #####   ######  # ## #  #     #  ######  #####   #    #  ######  #  ###  #
' #    #  #   #   #    #  ##  ##  #     #  #    #  #   #   #    #  #    #  #    #  #
' #####   #    #  #    #  #    #   #####   #    #  #    #  #####   #    #   ####   ######

sub drawGarbage (ctx as context_type, g as garbage_type, camera as point_type)
    if g.life = 0 then exit sub
    dim p as point_type
    p.x = g.position.x + camera.x
    p.y = g.position.y + camera.y
    normalizeWorldPosition p, ctx.world
    if innerDrawLimit(ctx, p) then pset (p.x, p.y), g.rgbaColor
    'circle (g.position.x,g.position.y), g.radius, g.rgbaColor
end sub

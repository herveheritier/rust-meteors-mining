$INCLUDEONCE

'$INCLUDE:'./world_type.bas'

type point_type
    x as double
    y as double
end type

sub setMinPoint(p as point_type)
    setPoint p, _DOUBLE_MIN#, _DOUBLE_MIN#
end sub

sub setMaxPoint(p as point_type)
    setPoint p, _DOUBLE_MAX#, _DOUBLE_MAX#
end sub

sub setPoint(p as point_type, x as double, y as double)
    p.x = x
    p.y = y
end sub

'                        ######                                                    # #
' #####    ####   #####  #     #  #####    ####   #####   #    #   ####   #####    # #
' #    #  #    #    #    #     #  #    #  #    #  #    #  #    #  #    #    #    #######
' #    #  #    #    #    ######   #    #  #    #  #    #  #    #  #         #      # #
' #    #  #    #    #    #        #####   #    #  #    #  #    #  #         #    #######
' #    #  #    #    #    #        #   #   #    #  #    #  #    #  #    #    #      # #
' #####    ####     #    #        #    #   ####   #####    ####    ####     #      # #

' scalar product

function dotProduct# (p as point_type, q as point_type)
    dotProduct# = p.x * q.x + p.y * q.y
end function

'                                                                    #     #                                  ######
' #    #   ####   #####   #    #    ##    #       #  ######  ######  #  #  #   ####   #####   #       #####   #     #   ####    ####   #  #####  #   ####   #    #
' ##   #  #    #  #    #  ##  ##   #  #   #       #      #   #       #  #  #  #    #  #    #  #       #    #  #     #  #    #  #       #    #    #  #    #  ##   #
' # #  #  #    #  #    #  # ## #  #    #  #       #     #    #####   #  #  #  #    #  #    #  #       #    #  ######   #    #   ####   #    #    #  #    #  # #  #
' #  # #  #    #  #####   #    #  ######  #       #    #     #       #  #  #  #    #  #####   #       #    #  #        #    #       #  #    #    #  #    #  #  # #
' #   ##  #    #  #   #   #    #  #    #  #       #   #      #       #  #  #  #    #  #   #   #       #    #  #        #    #  #    #  #    #    #  #    #  #   ##
' #    #   ####   #    #  #    #  #    #  ######  #  ######  ######   ## ##    ####   #    #  ######  #####   #         ####    ####   #    #    #   ####   #    #

sub normalizeWorldPosition (p as point_type, world as world_type)
    if p.x < world.minx then p.x = p.x - world.minx + world.maxx
    if p.x > world.maxx then p.x = p.x + world.minx - world.maxx
    if p.y < world.miny then p.y = p.y - world.miny + world.maxy
    if p.y > world.maxy then p.y = p.y + world.miny - world.maxy
end sub

'                                                                    ######                           ######
' #    #   ####   #####   #    #    ##    #       #  ######  ######  #     #  #         ##    #    #  #     #   ####    ####   #  #####  #   ####   #    #
' ##   #  #    #  #    #  ##  ##   #  #   #       #      #   #       #     #  #        #  #   ##   #  #     #  #    #  #       #    #    #  #    #  ##   #
' # #  #  #    #  #    #  # ## #  #    #  #       #     #    #####   ######   #       #    #  # #  #  ######   #    #   ####   #    #    #  #    #  # #  #
' #  # #  #    #  #####   #    #  ######  #       #    #     #       #        #       ######  #  # #  #        #    #       #  #    #    #  #    #  #  # #
' #   ##  #    #  #   #   #    #  #    #  #       #   #      #       #        #       #    #  #   ##  #        #    #  #    #  #    #    #  #    #  #   ##
' #    #   ####   #    #  #    #  #    #  ######  #  ######  ######  #        ######  #    #  #    #  #         ####    ####   #    #    #   ####   #    #

sub normalizePlanPosition (p as point_type, world as world_type, plan%)
    if p.x < (world.minx * plan%) then p.x = p.x - (world.minx * plan%) + (world.maxx * plan%)
    if p.x > (world.maxx * plan%) then p.x = p.x + (world.minx * plan%) - (world.maxx * plan%)
    if p.y < (world.miny * plan%) then p.y = p.y - (world.miny * plan%) + (world.maxy * plan%)
    if p.y > (world.maxy * plan%) then p.y = p.y + (world.miny * plan%) - (world.maxy * plan%)
end sub

'                         ######                                     #######                                  ###   #
'   ##    #####   ######  #     #   ####   #  #    #  #####   ####   #         ####   #    #    ##    #       # #  #
'  #  #   #    #  #       #     #  #    #  #  ##   #    #    #       #        #    #  #    #   #  #   #       ### #
' #    #  #    #  #####   ######   #    #  #  # #  #    #     ####   #####    #    #  #    #  #    #  #          #
' ######  #####   #       #        #    #  #  #  # #    #         #  #        #  # #  #    #  ######  #         # ###
' #    #  #   #   #       #        #    #  #  #   ##    #    #    #  #        #   #   #    #  #    #  #        #  # #
' #    #  #    #  ######  #         ####   #  #    #    #     ####   #######   ### #   ####   #    #  ######  #   ###

' compare 2 points

function arePointsEqual% (p1 as point_type, p2 as point_type)
    const EPSILON = 0.0001
    arePointsEqual% = (abs(p1.x - p2.x) < EPSILON and abs(p1.y - p2.y) < EPSILON)
end function

'                                                                #     #                                         #######                                            #######
'  ####   ######  #    #  ######  #####     ##    #####  ######  #     #  ######  #####   #####  ######  #    #  #     #  #    #  #####   ####   #  #####   ######     #     #####   #    ##    #    #   ####   #       ######
' #    #  #       ##   #  #       #    #   #  #     #    #       #     #  #       #    #    #    #        #  #   #     #  #    #    #    #       #  #    #  #          #     #    #  #   #  #   ##   #  #    #  #       #
' #       #####   # #  #  #####   #    #  #    #    #    #####   #     #  #####   #    #    #    #####     ##    #     #  #    #    #     ####   #  #    #  #####      #     #    #  #  #    #  # #  #  #       #       #####
' #  ###  #       #  # #  #       #####   ######    #    #        #   #   #       #####     #    #         ##    #     #  #    #    #         #  #  #    #  #          #     #####   #  ######  #  # #  #  ###  #       #
' #    #  #       #   ##  #       #   #   #    #    #    #         # #    #       #   #     #    #        #  #   #     #  #    #    #    #    #  #  #    #  #          #     #   #   #  #    #  #   ##  #    #  #       #
'  ####   ######  #    #  ######  #    #  #    #    #    ######     #     ######  #    #    #    ######  #    #  #######   ####     #     ####   #  #####   ######     #     #    #  #  #    #  #    #   ####   ######  ######

' create a vertex outside the triangle

sub generateVertexOutsideTriangle (p1 as point_type, p2 as point_type, p3 as point_type, h as double, r as point_type)
    dim result as point_type
    dim v as point_type ' ab vector
    dim l as double
    dim u as point_type ' normal unit vetor to ab
    dim side as double

    v.x = p2.x - p1.x
    v.y = p2.y - p1.y
    l = _hypot(v.x, v.y)
    if l = 0 then
        r.x = 0: r.y = 0
        exit sub
    end if

    ' normal vector perpendicular to ab : (-vy, vx)
    u.x = -v.y / l
    u.y = v.x / l

    ' determine which side of the line ab is c
    ' sign of (ac x ab) (2D vector product) : (cx-x1,cy-y1) x (vx,vy) = (cx-x1)*vy - (cy-y1)*vx
    side = (p3.x - p1.x) * v.y - (p3.y - p1.y) * v.x

    ' if side > 0 then it is on the side where the normal vector (-vy, vx) gives a positive sign
    'to place p in the half-plane opposite c, then reverse the normal if necessary.
    if side <= 0 then
        u.x = -u.x
        u.y = -u.y
    end if

    ' choose the midpoint of ab as the base for measuring the height
    r.x = (p1.x + p2.x) / 2 + u.x * h
    r.y = (p1.y + p2.y) / 2 + u.y * h

end sub

' #####    ####   #####    ##    #####  #   ####   #    #
' #    #  #    #    #     #  #     #    #  #    #  ##   #
' #    #  #    #    #    #    #    #    #  #    #  # #  #
' #####   #    #    #    ######    #    #  #    #  #  # #
' #   #   #    #    #    #    #    #    #  #    #  #   ##
' #    #   ####     #    #    #    #    #   ####   #    #

sub rotation (a as point_type, axe as point_type, angle as double)
    ax0 = a.x - axe.x
    ay0 = a.y - axe.y
    a.x = ax0 * cos(angle) - ay0 * sin(angle) + axe.x
    a.y = ax0 * sin(angle) + ay0 * cos(angle) + axe.y
end sub

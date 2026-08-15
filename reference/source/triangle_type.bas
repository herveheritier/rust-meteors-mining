$INCLUDEONCE

'$INCLUDE:'./point_type.bas'
'$INCLUDE:'./segment_type.bas'

type triangle_type
    id as integer
    position as point_type
    angle as double
    hauteur as double
    demibase as point_type
    a as point_type
    b as point_type
    c as point_type
    center as point_type
    realA as point_type
    realB as point_type
    realC as point_type
    realCenter as point_type
    realMin as point_type
    realMax as point_type
    ''''''''''''''''''''''''''''''
    collid as integer
    collidBy as integer
    life as integer
    ''''''''''''''''''''''''''''''
    shapeIndex as integer
    element as integer
    aShapeBorder as integer
    bShapeBorder as integer
    cShapeBorder as integer
    ''''''''''''''''''''''''''''''
    textureBasePosition as integer
end type

'                                                                    #####                                              ###   #
' #####  #####   #    ##    #    #   ####   #       ######   ####   #     #   ####   #       #       #  #####   ######  # #  #
'   #    #    #  #   #  #   ##   #  #    #  #       #       #       #        #    #  #       #       #  #    #  #       ### #
'   #    #    #  #  #    #  # #  #  #       #       #####    ####   #        #    #  #       #       #  #    #  #####      #
'   #    #####   #  ######  #  # #  #  ###  #       #            #  #        #    #  #       #       #  #    #  #         # ###
'   #    #   #   #  #    #  #   ##  #    #  #       #       #    #  #     #  #    #  #       #       #  #    #  #        #  # #
'   #    #    #  #  #    #  #    #   ####   ######  ######   ####    #####    ####   ######  ######  #  #####   ######  #   ###

' test triangles collision

function trianglesCollide% (A as triangle_type, B as triangle_type)
    dim t as integer, i as integer, j as integer
    dim p1 as point_type, p2 as point_type
    dim edge as point_type, axis as point_type
    dim minA as double, maxA as double, minB as double, maxB as double
    dim vertsA(2) as point_type, vertsB(2) as point_type
    const INF = 1E+308
    const EPS = 1E-9

    ' vertices
    vertsA(0) = A.realA: vertsA(1) = A.realB: vertsA(2) = A.realC
    vertsB(0) = B.realA: vertsB(1) = B.realB: vertsB(2) = B.realC

    ' triangles comparaison
    for t = 0 to 1
        for i = 0 to 2
            if t = 0 then
                p1 = vertsA(i)
                p2 = vertsA((i + 1) mod 3)
            else
                p1 = vertsB(i)
                p2 = vertsB((i + 1) mod 3)
            end if

            edge.x = p2.x - p1.x
            edge.y = p2.y - p1.y
            axis.x = -edge.y
            axis.y = edge.x

            ' projection A triangle
            minA = INF: maxA = -INF
            for j = 0 to 2
                proj = dotProduct(vertsA(j), axis)
                if proj < minA then minA = proj
                if proj > maxA then maxA = proj
            next j

            ' projection B triangle
            minB = INF: maxB = -INF
            for j = 0 to 2
                proj = dotProduct(vertsB(j), axis)
                if proj < minB then minB = proj
                if proj > maxB then maxB = proj
            next j

            ' no intersection beetween A and B
            if maxA < minB - EPS or maxB < minA - EPS then
                trianglesCollide% = 0
                exit function
            end if
        next i
    next t

    ' A and B intersect
    trianglesCollide% = -1
end function

'             #####                                                   #####                                           ###   #
' #   ####   #     #  ######   ####   #    #  ######  #    #  #####  #     #  #    #    ##    #####   ######  #####   # #  #
' #  #       #        #       #    #  ##  ##  #       ##   #    #    #        #    #   #  #   #    #  #       #    #  ### #
' #   ####    #####   #####   #       # ## #  #####   # #  #    #     #####   ######  #    #  #    #  #####   #    #     #
' #       #        #  #       #  ###  #    #  #       #  # #    #          #  #    #  ######  #####   #       #    #    # ###
' #  #    #  #     #  #       #    #  #    #  #       #   ##    #    #     #  #    #  #    #  #   #   #       #    #   #  # #
' #   ####    #####   ######   ####   #    #  ######  #    #    #     #####   #    #  #    #  #    #  ######  #####   #   ###

' check if a segment belongs to the triangle

function isSegmentShared% (s as segment_type, t as triangle_type)
    ' Vérifie si le segment appartient au triangle
    if (arePointsEqual(s.a, t.a) and arePointsEqual(s.b, t.b)) _orelse _
       (arePointsEqual(s.a, t.b) and arePointsEqual(s.b, t.c)) _orelse _
       (arePointsEqual(s.a, t.c) and arePointsEqual(s.b, t.a)) _orelse _
       (arePointsEqual(s.b, t.a) and arePointsEqual(s.a, t.b)) _orelse _
       (arePointsEqual(s.b, t.b) and arePointsEqual(s.a, t.c)) _orelse _
       (arePointsEqual(s.b, t.c) and arePointsEqual(s.a, t.a)) then
        isSegmentShared% = -1
    else
        isSegmentShared% = 0
    end if
end function

'            #     #                                         ###                                  #######                                                     ###   #
' #   ####   #     #  ######  #####   #####  ######  #    #   #   #    #  #    #  ######  #####      #     #####   #    ##    #    #   ####   #       ######  # #  #
' #  #       #     #  #       #    #    #    #        #  #    #   ##   #  ##   #  #       #    #     #     #    #  #   #  #   ##   #  #    #  #       #       ### #
' #   ####   #     #  #####   #    #    #    #####     ##     #   # #  #  # #  #  #####   #    #     #     #    #  #  #    #  # #  #  #       #       #####      #
' #       #   #   #   #       #####     #    #         ##     #   #  # #  #  # #  #       #####      #     #####   #  ######  #  # #  #  ###  #       #         # ###
' #  #    #    # #    #       #   #     #    #        #  #    #   #   ##  #   ##  #       #   #      #     #   #   #  #    #  #   ##  #    #  #       #        #  # #
' #   ####      #     ######  #    #    #    ######  #    #  ###  #    #  #    #  ######  #    #     #     #    #  #  #    #  #    #   ####   ######  ######  #   ###

' check if a vertex is inside the triangle

function isVertexInnerTriangle% (triangle as triangle_type, vertex as point_type)
    dim v0 as point_type
    dim v1 as point_type
    dim v2 as point_type
    dim dot00 as double, dot01 as double, dot02 as double, dot11 as double, dot12 as double
    dim invdenom as double
    dim u as double, v as double

    ' vectors
    v0.x = triangle.c.x - triangle.a.x: v0.y = triangle.c.y - triangle.a.y
    v1.x = triangle.b.x - triangle.a.x: v1.y = triangle.b.y - triangle.a.y
    v2.x = vertex.x - triangle.a.x: v2.y = vertex.y - triangle.a.y

    ' scalar products
    dot00 = v0.x * v0.x + v0.y * v0.y
    dot01 = v0.x * v1.x + v0.y * v1.y
    dot02 = v0.x * v2.x + v0.y * v2.y
    dot11 = v1.x * v1.x + v1.y * v1.y
    dot12 = v1.x * v2.x + v1.y * v2.y

    ' centroid
    invdenom = dot00 * dot11 - dot01 * dot01
    if abs(invdenom) < 1E-12 then
        isVertexInnerTriangle% = 0 ' collinear
        exit function
    end if

    invdenom = 1# / invdenom
    u = (dot11 * dot02 - dot01 * dot12) * invdenom
    v = (dot00 * dot12 - dot01 * dot02) * invdenom

    ' point inside only if u>=0, v>=0 and u+v<=1
    if u >= 0 - 1E-12 and v >= 0 - 1E-12 and u + v <= 1 + 1E-12 then
        isVertexInnerTriangle% = -1
    else
        isVertexInnerTriangle% = 0 ' à l'extérieur
    end if
end function

'                                                                #######
'  ####   ######  #    #  ######  #####     ##    #####  ######     #     #####   #    ##    #    #   ####   #       ######
' #    #  #       ##   #  #       #    #   #  #     #    #          #     #    #  #   #  #   ##   #  #    #  #       #
' #       #####   # #  #  #####   #    #  #    #    #    #####      #     #    #  #  #    #  # #  #  #       #       #####
' #  ###  #       #  # #  #       #####   ######    #    #          #     #####   #  ######  #  # #  #  ###  #       #
' #    #  #       #   ##  #       #   #   #    #    #    #          #     #   #   #  #    #  #   ##  #    #  #       #
'  ####   ######  #    #  ######  #    #  #    #    #    ######     #     #    #  #  #    #  #    #   ####   ######  ######

' generate a new triangle

sub generateTriangle (t as triangle_type, baseMin%, baseMax%, hauteurMin%, hauteurMax%)
    bas = baseMax% - rnd * (baseMax% - baseMin%)
    angle = rnd * TAU
    t.a.x = 0
    t.a.y = 0
    t.b.x = cos(angle) * bas
    t.b.y = -sin(angle) * bas
    hauteur = hauteurMax% - rnd * (hauteurMax% - hauteurMin%)
    dim demibase as point_type
    demibase.x = t.b.x / 2
    demibase.y = t.b.y / 2
    t.c.x = demibase.x + cos(angle + TAU / 4) * hauteur
    t.c.y = demibase.y - sin(angle + TAU / 4) * hauteur
    t.center.x = (t.a.x + t.b.x + t.c.x) / 3
    t.center.y = (t.a.y + t.b.y + t.c.y) / 3
    t.life = 1
end sub

'                                                #######
'  ####   #####   ######    ##    #####  ######     #     #####   #    ##    #    #   ####   #       ######
' #    #  #    #  #        #  #     #    #          #     #    #  #   #  #   ##   #  #    #  #       #
' #       #    #  #####   #    #    #    #####      #     #    #  #  #    #  # #  #  #       #       #####
' #       #####   #       ######    #    #          #     #####   #  ######  #  # #  #  ###  #       #
' #    #  #   #   #       #    #    #    #          #     #   #   #  #    #  #   ##  #    #  #       #
'  ####   #    #  ######  #    #    #    ######     #     #    #  #  #    #  #    #   ####   ######  ######

' create a triangle with 3 choosen vertex

sub createTriangle (t as triangle_type, p1 as point_type, p2 as point_type, p3 as point_type)
    t.a = p1
    t.b = p2
    t.c = p3
    t.position.x = 0
    t.position.y = 0
    t.angle = _atan2(t.b.y - t.a.y, t.b.x - t.a.x)
    t.hauteur = abs((t.b.x - t.a.x) * (t.a.y - t.c.y) - (t.b.y - t.a.y) * (t.a.x - t.c.x)) / _hypot((t.b.x - t.a.x), (t.b.y - t.a.y))
    t.demibase.x = (t.b.x - t.a.x) / 2
    t.demibase.y = (t.b.y - t.a.y) / 2
    t.center.x = (t.a.x + t.b.x + t.c.x) / 3
    t.center.y = (t.a.y + t.b.y + t.c.y) / 3
    t.life = 1
end sub

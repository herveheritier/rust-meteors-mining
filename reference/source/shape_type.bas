$INCLUDEONCE

'$INCLUDE:'./point_type.bas'
'$INCLUDE:'./triangle_type.bas'

type shape_type
    id as integer
    firstTriangleIndex as integer
    lastTriangleIndex as integer
    pointsUsageIndicator as string
    position as point_type
    ''''''''''''''''''''''''''''''
    width as double
    height as double
    topLeft as point_type
    bottomRight as point_type
    center as point_type
    targetCenter as point_type
    ''''''''''''''''''''''''''''''
    radius as double
    direction as double
    velocity as double
    orientation as double
    rotation as double
    shapeColor as _unsigned long
    texture as _unsigned long
    ''''''''''''''''''''''''''''''
    isCollider as integer
    life as integer
    element as integer
    showAllParts as integer
    whoIam as integer ' identify what is the shape
    ''''
end type

'                                  #####                                   ###   #
' ######  #####   ######  ######  #     #  #    #    ##    #####   ######  # #  #
' #       #    #  #       #       #        #    #   #  #   #    #  #       ### #
' #####   #    #  #####   #####    #####   ######  #    #  #    #  #####      #
' #       #####   #       #             #  #    #  ######  #####   #         # ###
' #       #   #   #       #       #     #  #    #  #    #  #       #        #  # #
' #       #    #  ######  ######   #####   #    #  #    #  #       ######  #   ###

' find a destroyed shape with a particular number of triangles

function freeShape% (shapes() as shape_type, nbr%)
    if ubound(shapes) > 2 then
        for i% = 1 to ubound(shapes)
            if shapes(i%).life > 0 then _continue
            if (shapes(i%).lastTriangleIndex - shapes(i%).firstTriangleIndex + 1) = nbr% then
                freeShape% = i%
                exit function
            end if
        next i%
    end if
    freeShape% = -1
end function

'                                                         #######                                             #####
' #####   ######   ####    ####   #       #    #  ######  #        #         ##     ####   #####  #   ####   #     #   ####   #       #       #   ####   #   ####   #    #
' #    #  #       #       #    #  #       #    #  #       #        #        #  #   #         #    #  #    #  #        #    #  #       #       #  #       #  #    #  ##   #
' #    #  #####    ####   #    #  #       #    #  #####   #####    #       #    #   ####     #    #  #       #        #    #  #       #       #   ####   #  #    #  # #  #
' #####   #            #  #    #  #       #    #  #       #        #       ######       #    #    #  #       #        #    #  #       #       #       #  #  #    #  #  # #
' #   #   #       #    #  #    #  #        #  #   #       #        #       #    #  #    #    #    #  #    #  #     #  #    #  #       #       #  #    #  #  #    #  #   ##
' #    #  ######   ####    ####   ######    ##    ######  #######  ######  #    #   ####     #    #   ####    #####    ####   ######  ######  #   ####   #   ####   #    #

sub resolveElasticCollision (a as shape_type, b as shape_type)

    dim d as point_type

    ' positions relatives
    d.x = b.position.x - a.position.x
    d.y = b.position.y - a.position.y
    dist = _hypot(d.x, d.y)

    ' vérifier collision (recouvrement)
    if dist >= (a.radius + b.radius) then
        exit sub
    end if

    ' vecteur normal unitaire (de A vers B)
    if dist = 0 then
        nx = 1.0
        ny = 0.0
    else
        nx = d.x / dist
        ny = d.y / dist
    end if

    ' convertir vitesses polaires (direction en radians) en composantes cartésiennes
    ax = a.velocity * cos(a.direction)
    ay = a.velocity * sin(a.direction)
    bx = b.velocity * cos(b.direction)
    by = b.velocity * sin(b.direction)

    ' projections sur la normale (scalaire)
    va_n = ax * nx + ay * ny
    vb_n = bx * nx + by * ny

    ' vecteur tangent unitaire (rotated normal)
    tx = -ny
    ty = nx
    va_t = ax * tx + ay * ty
    vb_t = bx * tx + by * ty

    ' choc élastique 1D sur la composante normale
    ma = a.lastTriangleIndex - a.firstTriangleIndex
    mb = b.lastTriangleIndex - b.firstTriangleIndex
    va_n_after = (va_n * (ma - mb) + 2# * mb * vb_n) / (ma + mb)
    vb_n_after = (vb_n * (mb - ma) + 2# * ma * va_n) / (ma + mb)

    ' recomposer vecteurs vitesse après collision
    ax_after = va_n_after * nx + va_t * tx
    ay_after = va_n_after * ny + va_t * ty
    bx_after = vb_n_after * nx + vb_t * tx
    by_after = vb_n_after * ny + vb_t * ty

    ' convertir en polaires (vitesse et direction en radians)
    a.velocity = _hypot(ax_after, ay_after)
    if a.velocity = 0 then
        a.direction = 0
    else
        a.direction = _atan2(ay_after, ax_after) ' déjà en radians
        if a.direction < 0 then a.direction = a.direction + TAU
    end if

    b.velocity = _hypot(bx_after, by_after)
    if b.velocity = 0 then
        b.direction = 0
    else
        b.direction = _atan2(by_after, bx_after)
        if b.direction < 0 then b.direction = b.direction + TAU
    end if

end sub

'                                                #####                                                         ###   #
' #####   ######  #####  ######   ####   #####  #     #   ####   #       #       #   ####   #   ####   #    #  # #  #
' #    #  #         #    #       #    #    #    #        #    #  #       #       #  #       #  #    #  ##   #  ### #
' #    #  #####     #    #####   #         #    #        #    #  #       #       #   ####   #  #    #  # #  #     #
' #    #  #         #    #       #         #    #        #    #  #       #       #       #  #  #    #  #  # #    # ###
' #    #  #         #    #       #    #    #    #     #  #    #  #       #       #  #    #  #  #    #  #   ##   #  # #
' #####   ######    #    ######   ####     #     #####    ####   ######  ######  #   ####   #   ####   #    #  #   ###

' detect collision beetween two shapes

function detectCollision% (shapeA as shape_type, shapeB as shape_type, triangles() as triangle_type)
    res% = 0
    for i% = shapeA.firstTriangleIndex to shapeA.lastTriangleIndex
        if triangles(i%).life = 0 then _continue
        for j% = shapeB.firstTriangleIndex to shapeB.lastTriangleIndex
            if triangles(j%).life = 0 then _continue
            if triangles(j%).realMax.x < triangles(i%).realMin.x _orelse _
               triangles(j%).realMin.x > triangles(i%).realMax.x _orelse _
               triangles(j%).realMax.y < triangles(i%).realMin.y _orelse _
               triangles(j%).realMin.y > triangles(i%).realMax.y then
            else
                if trianglesCollide(triangles(j%), triangles(i%)) then

                    triangles(i%).collid = -1
                    triangles(i%).collidBy = shapeB.whoIam
                    'triangles(i%).life = 0
                    'shapeA.life = shapeA.life - 1

                    triangles(j%).collid = -1
                    triangles(j%).collidBy = shapeA.whoIam
                    'triangles(j%).life = 0
                    'shapeB.life = shapeB.life - 1

                    res% = -1
                end if
            end if
        next j%
    next i%
    detectCollision% = res%
end function

'                                             #####
' #    #   ####   #    #  #  #    #   ####   #     #  #    #    ##    #####   ######
' ##  ##  #    #  #    #  #  ##   #  #    #  #        #    #   #  #   #    #  #
' # ## #  #    #  #    #  #  # #  #  #        #####   ######  #    #  #    #  #####
' #    #  #    #  #    #  #  #  # #  #  ###        #  #    #  ######  #####   #
' #    #  #    #   #  #   #  #   ##  #    #  #     #  #    #  #    #  #       #
' #    #   ####     ##    #  #    #   ####    #####   #    #  #    #  #       ######

' move and rotate shape

sub movingShape (shape as shape_type, triangles() as triangle_type, world as world_type, fps%)
    shape.position.x = shape.position.x + cos(shape.direction) * 60 * shape.velocity / fps%
    shape.position.y = shape.position.y - sin(shape.direction) * 60 * shape.velocity / fps%
    normalizeWorldPosition shape.position, world
    shape.center.x = shape.center.x + (shape.targetCenter.x - shape.center.x) / 100
    shape.center.y = shape.center.y + (shape.targetCenter.y - shape.center.y) / 100
    shape.orientation = shape.orientation + (60 * shape.rotation / fps%)
    for i% = shape.firstTriangleIndex to shape.lastTriangleIndex
        computeRealPositions triangles(i%), shape.position, shape.center, shape.orientation
    next i%
end sub

'                                                        ######                           ######
'  ####    ####   #    #  #####   #    #  #####  ######  #     #  ######    ##    #       #     #   ####    ####   #  #####  #   ####   #    #   ####
' #    #  #    #  ##  ##  #    #  #    #    #    #       #     #  #        #  #   #       #     #  #    #  #       #    #    #  #    #  ##   #  #
' #       #    #  # ## #  #    #  #    #    #    #####   ######   #####   #    #  #       ######   #    #   ####   #    #    #  #    #  # #  #   ####
' #       #    #  #    #  #####   #    #    #    #       #   #    #       ######  #       #        #    #       #  #    #    #  #    #  #  # #       #
' #    #  #    #  #    #  #       #    #    #    #       #    #   #       #    #  #       #        #    #  #    #  #    #    #  #    #  #   ##  #    #
'  ####    ####   #    #  #        ####     #    ######  #     #  ######  #    #  ######  #         ####    ####   #    #    #   ####   #    #   ####

' compute the screen position for a triangle base on shape position, shape center and shape orientation

sub computeRealPositions (t as triangle_type, p as point_type, axe as point_type, angle as double)
    dim as point_type a, b, c, center
    a.x = t.a.x + t.position.x
    a.y = t.a.y + t.position.y
    b.x = t.b.x + t.position.x
    b.y = t.b.y + t.position.y
    c.x = t.c.x + t.position.x
    c.y = t.c.y + t.position.y
    center.x = t.center.x + t.position.x
    center.y = t.center.y + t.position.y
    rotation a, axe, angle
    rotation b, axe, angle
    rotation c, axe, angle
    rotation center, axe, angle
    t.realA.x = p.x + a.x
    t.realA.y = p.y + a.y
    t.realB.x = p.x + b.x
    t.realB.y = p.y + b.y
    t.realC.x = p.x + c.x
    t.realC.y = p.y + c.y
    t.realCenter.x = p.x + center.x
    t.realCenter.y = p.y + center.y
    t.realMin.x = _min(_min(t.realA.x, t.realB.x), t.realC.x)
    t.realMin.y = _min(_min(t.realA.y, t.realB.y), t.realC.y)
    t.realMax.x = _max(_max(t.realA.x, t.realB.x), t.realC.x)
    t.realMax.y = _max(_max(t.realA.y, t.realB.y), t.realC.y)
end sub

'                                                         #####                                    #####
'  ####    ####   #    #  #####   #    #  #####  ######  #     #  #    #    ##    #####   ######  #     #  ######  #    #  #####  ######  #####
' #    #  #    #  ##  ##  #    #  #    #    #    #       #        #    #   #  #   #    #  #       #        #       ##   #    #    #       #    #
' #       #    #  # ## #  #    #  #    #    #    #####    #####   ######  #    #  #    #  #####   #        #####   # #  #    #    #####   #    #
' #       #    #  #    #  #####   #    #    #    #             #  #    #  ######  #####   #       #        #       #  # #    #    #       #####
' #    #  #    #  #    #  #       #    #    #    #       #     #  #    #  #    #  #       #       #     #  #       #   ##    #    #       #   #
'  ####    ####   #    #  #        ####     #    ######   #####   #    #  #    #  #       ######   #####   ######  #    #    #    ######  #    #

' compute shape center base on active triangles

sub computeShapeCenter (shape as shape_type, triangles() as triangle_type)
    dim as double x, y, radius
    dim p as point_type
    if shape.life <= 0 then exit sub
    d% = 0
    for i% = shape.firstTriangleIndex to shape.lastTriangleIndex
        if triangles(i%).life <= 0 then _continue
        if triangles(i%).life <= 0 then _continue
        d% = d% + 1
        x = x + (triangles(i%).a.x + triangles(i%).b.x + triangles(i%).c.x) / 3
        y = y + (triangles(i%).a.y + triangles(i%).b.y + triangles(i%).c.y) / 3
    next i%
    p.x = x / d%
    p.y = y / d%
    shape.targetCenter.x = p.x
    shape.targetCenter.y = p.y

    radius = 0
    for i% = shape.firstTriangleIndex to shape.lastTriangleIndex
        if triangles(i%).life <= 0 then _continue
        h = _hypot( triangles(i%).center.x - shape.targetCenter.x, triangles(i%).center.y - shape.targetCenter.y ) _
            + triangles(i%).hauteur
        radius = _max(radius, h)
    next i%
    shape.radius = radius

    setMaxPoint shape.topLeft
    setMinPoint shape.bottomRight
    for i% = shape.firstTriangleIndex to shape.lastTriangleIndex
        minx# = _min(triangles(i%).a.x,_min(triangles(i%).b.x,triangles(i%).c.x))
        if shape.topLeft.x > minx# then shape.topLeft.x = minx#
        miny# = _min(triangles(i%).a.y,_min(triangles(i%).b.y,triangles(i%).c.y))
        if shape.topLeft.y > miny# then shape.topLeft.y = miny#
        maxx# = _max(triangles(i%).a.x,_max(triangles(i%).b.x,triangles(i%).c.x))
        if shape.bottomRight.x < maxx# then shape.bottomRight.x = maxx#
        maxy# = _max(triangles(i%).a.y,_max(triangles(i%).b.y,triangles(i%).c.y))
        if shape.bottomRight.y < maxy# then shape.bottomRight.y = maxy#
    next i%

    shape.width = shape.bottomRight.x - shape.topLeft.x
    shape.height = shape.bottomRight.y - shape.topLeft.y

    larger = _iif(shape.width>shape.height,shape.width,shape.height)
    tw% = _width(shape.texture)
    ratio =  tw% / larger

    'sh& = _dest : _dest _console : ? larger, tw%, ratio : _dest h&
end sub

'                        ######                                            #####
'  ####   ######  #####  #     #   ####   #####   #####   ######  #####   #     #  ######   ####   #    #  ######  #    #  #####   ####
' #    #  #         #    #     #  #    #  #    #  #    #  #       #    #  #        #       #    #  ##  ##  #       ##   #    #    #
' #       #####     #    ######   #    #  #    #  #    #  #####   #    #   #####   #####   #       # ## #  #####   # #  #    #     ####
' #  ###  #         #    #     #  #    #  #####   #    #  #       #####         #  #       #  ###  #    #  #       #  # #    #         #
' #    #  #         #    #     #  #    #  #   #   #    #  #       #   #   #     #  #       #    #  #    #  #       #   ##    #    #    #
'  ####   ######    #    ######    ####   #    #  #####   ######  #    #   #####   ######   ####   #    #  ######  #    #    #     ####

' get boundary edges (border segments) of a shape

sub getBorderSegments (shape as shape_type, triangles() as triangle_type)
    dim segmentCount%
    dim shard%
    dim s(1 to 3) as segment_type

    ' explore every triangle of the shape
    for i% = shape.firstTriangleIndex to shape.lastTriangleIndex

        if triangles(i%).life <= 0 then _continue

        triangles(i%).aShapeBorder = 0
        triangles(i%).bShapeBorder = 0
        triangles(i%).cShapeBorder = 0

        ' get the 3 segments of the triangle
        s(1).a = triangles(i%).a: s(1).b = triangles(i%).b
        s(2).a = triangles(i%).b: s(2).b = triangles(i%).c
        s(3).a = triangles(i%).c: s(3).b = triangles(i%).a

        ' test segments
        for j% = 1 to 3

            shard% = 0

            ' a common segment with another triangle is not a boundary edge of the shape
            for k% = shape.firstTriangleIndex to shape.lastTriangleIndex
                if k% = i% then _continue
                if triangles(k%).life <= 0 then _continue
                shard% = isSegmentShared(s(j%), triangles(k%))
                if shard% then
                    exit for
                end if
            next k%
            
            ' if the segment is not shared, it is a boundary edge.
            if j% = 1 and shard% = 0 then triangles(i%).aShapeBorder = -1
            if j% = 2 and shard% = 0 then triangles(i%).bShapeBorder = -1
            if j% = 3 and shard% = 0 then triangles(i%).cShapeBorder = -1
        next j%
    next i%

end sub

'            #######                                                     #     #                             ###   #
' #   ####      #     #####   #    ##    #    #   ####   #       ######  #     #    ##    #       #  #####   # #  #
' #  #          #     #    #  #   #  #   ##   #  #    #  #       #       #     #   #  #   #       #  #    #  ### #
' #   ####      #     #    #  #  #    #  # #  #  #       #       #####   #     #  #    #  #       #  #    #     #
' #       #     #     #####   #  ######  #  # #  #  ###  #       #        #   #   ######  #       #  #    #    # ###
' #  #    #     #     #   #   #  #    #  #   ##  #    #  #       #         # #    #    #  #       #  #    #   #  # #
' #   ####      #     #    #  #  #    #  #    #   ####   ######  ######     #     #    #  ######  #  #####   #   ###

' check whether a new specific triangle can be added to the shape without covering an other triangle in the shape

function isTriangleValid% (shape as shape_type, triangles() as triangle_type, triangle as triangle_type)
    dim s2(1 to 3) as segment_type
    dim s1(1 to 3) as segment_type
    ' Prepare the triangle segments to be tested
    s2(1).a = triangle.a: s2(1).b = triangle.b
    s2(2).a = triangle.b: s2(2).b = triangle.c
    s2(3).a = triangle.c: s2(3).b = triangle.a

    for i% = shape.firstTriangleIndex to shape.lastTriangleIndex
        ' Prepare the segments of the existing triangle
        s1(1).a = triangles(i%).a: s1(1).b = triangles(i%).b
        s1(2).a = triangles(i%).b: s1(2).b = triangles(i%).c
        s1(3).a = triangles(i%).c: s1(3).b = triangles(i%).a

        ' Checks segments intersection
        for k% = 1 to 3
            for l% = 1 to 3
                if checkSegmentsIntersect(s1(k%), s2(l%), ix, iy) = -1 then
                    isTriangleValid% = 0
                    exit function
                end if
            next l%
        next k%
    next i%
    isTriangleValid% = -1
end function

'            #     #                                         ###                                   #####                                   ###   #
' #   ####   #     #  ######  #####   #####  ######  #    #   #   #    #  #    #  ######  #####   #     #  #    #    ##    #####   ######  # #  #
' #  #       #     #  #       #    #    #    #        #  #    #   ##   #  ##   #  #       #    #  #        #    #   #  #   #    #  #       ### #
' #   ####   #     #  #####   #    #    #    #####     ##     #   # #  #  # #  #  #####   #    #   #####   ######  #    #  #    #  #####      #
' #       #   #   #   #       #####     #    #         ##     #   #  # #  #  # #  #       #####         #  #    #  ######  #####   #         # ###
' #  #    #    # #    #       #   #     #    #        #  #    #   #   ##  #   ##  #       #   #   #     #  #    #  #    #  #       #        #  # #
' #   ####      #     ######  #    #    #    ######  #    #  ###  #    #  #    #  ######  #    #   #####   #    #  #    #  #       ######  #   ###

' check if a vertex is inside the shape

function isVertexInnerShape% (shape as shape_type, triangles() as triangle_type, vertex as point_type)
    for i% = shape.firstTriangleIndex to shape.lastTriangleIndex
        if isVertexInnerTriangle(triangles(i%), vertex) then
            isVertexInnerShape% = -1
            exit function
        end if
    next i%
    isVertexInnerShape% = 0
end function

'                                                 ######                                            #####                                                  ###   #
'  ####   #    #   ####    ####    ####   ######  #     #   ####   #####   #####   ######  #####   #     #  ######   ####   #    #  ######  #    #  #####  # #  #
' #    #  #    #  #    #  #    #  #       #       #     #  #    #  #    #  #    #  #       #    #  #        #       #    #  ##  ##  #       ##   #    #    ### #
' #       ######  #    #  #    #   ####   #####   ######   #    #  #    #  #    #  #####   #    #   #####   #####   #       # ## #  #####   # #  #    #       #
' #       #    #  #    #  #    #       #  #       #     #  #    #  #####   #    #  #       #####         #  #       #  ###  #    #  #       #  # #    #      # ###
' #    #  #    #  #    #  #    #  #    #  #       #     #  #    #  #   #   #    #  #       #   #   #     #  #       #    #  #    #  #       #   ##    #     #  # #
'  ####   #    #   ####    ####    ####   ######  ######    ####   #    #  #####   ######  #    #   #####   ######   ####   #    #  ######  #    #    #    #   ###

' select a free border segment

function chooseBorderSegment% (shape as shape_type, triangles() as triangle_type)
    l% = len(shape.pointsUsageIndicator) + 1
    i% = rnd * l%
    do
        bs% = instr(i%, shape.pointsUsageIndicator, "0")
        i% = (i% + 1) mod l%
    loop until bs% > 0
    mid$(shape.pointsUsageIndicator, bs%, 1) = "1"
    chooseBorderSegment% = bs% - 1
end function

'                                   #######
' #####   #####   #  #    #  #####     #     #####   #    ##    #    #   ####   #       ######
' #    #  #    #  #  ##   #    #       #     #    #  #   #  #   ##   #  #    #  #       #
' #    #  #    #  #  # #  #    #       #     #    #  #  #    #  # #  #  #       #       #####
' #####   #####   #  #  # #    #       #     #####   #  ######  #  # #  #  ###  #       #
' #       #   #   #  #   ##    #       #     #   #   #  #    #  #   ##  #    #  #       #
' #       #    #  #  #    #    #       #     #    #  #  #    #  #    #   ####   ######  ######

sub printTriangle (t as triangle_type, shapes() as shape_type, force%)
    dim s as shape_type
    s = shapes(t.shapeIndex)
    if t.id = s.firstTriangleIndex _orelse force% then
        print "--------------------------------------------------------------"
        print "shape - id:"; s.id, " - triangles:"; s.firstTriangleIndex; ">"; s.lastTriangleIndex
        print "life:"; s.life; "- element:"; s.element; " - whoiam:"; s.whoIam
        print "pointsUsageIndicator:"; s.pointsUsageIndicator
        print "position:("; trunc(s.position.x, 2); ","; trunc(s.position.y, 2); ")",
        print "- center:("; trunc(s.center.x, 2); ","; trunc(s.center.y, 2); ")",
        print "- targetCenter:("; trunc(s.center.x, 2); ","; trunc(s.center.y, 2); ")"
        print "radius:"; trunc(s.radius, 2); "- direction:"; deg(s.direction); "- velocity:"; trunc(s.velocity, 2)
        print "orientation:"; deg(s.orientation); "-rotation:"; deg(s.rotation)
    end if
    print "- - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -"
    print "triangle - id:"; t.id, "- shapeIndex:"; t.shapeIndex, "- element:"; t.element, "- life:"; t.life
    print "position:"; t.position.x; ","; t.position.y,
    print " - angle:"; deg(t.angle), "- hauteur:"; trunc(t.hauteur, 2),
    print " - demibase:("; trunc(t.demibase.x, 2); ","; trunc(t.demibase.y, 2); ")"
    print "vertex:("; trunc(t.a.x, 2); ","; trunc(t.a.y, 2);
    print ")-("; trunc(t.b.x, 2); ","; trunc(t.b.y, 2); ")-("; trunc(t.c.x, 2); ","; trunc(t.c.y, 2); ")"
    print "center:("; trunc(t.center.x, 2); ","; trunc(t.center.y, 2); ")"
end sub

'                                           #######         #####
' #    # ######  ####  #    # ######  ####     #     ####  #     # #    #   ##   #####  ######
' ##  ## #      #      #    # #      #         #    #    # #       #    #  #  #  #    # #
' # ## # #####   ####  ###### #####   ####     #    #    #  #####  ###### #    # #    # #####
' #    # #           # #    # #           #    #    #    #       # #    # ###### #####  #
' #    # #      #    # #    # #      #    #    #    #    # #     # #    # #    # #      #
' #    # ######  ####  #    # ######  ####     #     ####   #####  #    # #    # #      ######

sub meshesToShape (shape as shape_type, shapes() as shape_type, triangles() as triangle_type)

    dim as point_type p1, p2, p3
    dim t as triangle_type

    read packIndex%
    dim packSize(1 to packIndex%) as integer
    pointsQty% = 0

    for i% = 1 to packIndex%
        read packSize(i%)
        pointsQty% = pointsQty% + packSize(i%)
    next i%

    shapeIndex% = freeShape(shapes(), pointsQty%)

    if shapeIndex% > 0 then
        shape = shapes(shapeIndex%)
    else
        redim _preserve shapes(ubound(shapes) + 1) as shape_type
        shapeIndex% = ubound(shapes)
        redim _preserve triangles(0 to ubound(triangles) + pointsQty%) as triangle_type
        shape.lastTriangleIndex = ubound(triangles)
        shape.firstTriangleIndex = shape.lastTriangleIndex - pointsQty% + 1
    end if

    shape.id = shapeIndex%
    shape.life = pointsQty%

    for i% = 1 to packIndex%
        read p1.x, p1.y, p2.x, p2.y
        for j% = 1 to (packSize(i%) - 2)
            read p3.x, p3.y
            createTriangle t, p1, p2, p3
            t.shapeIndex = shapeIndex%
            pointsQty% = pointsQty% - 1
            t.id = shape.lastTriangleIndex - pointsQty%
            triangles(t.id) = t
            p1 = p2: p2 = p3
        next j%
    next i%
end sub

'                                       #####
' #####  ######  ####  # ###### ###### #     # #    #   ##   #####  ######
' #    # #      #      #     #  #      #       #    #  #  #  #    # #
' #    # #####   ####  #    #   #####   #####  ###### #    # #    # #####
' #####  #           # #   #    #            # #    # ###### #####  #
' #   #  #      #    # #  #     #      #     # #    # #    # #      #
' #    # ######  ####  # ###### ######  #####  #    # #    # #      ######

sub resizeShape (resizeFactor!, shape as shape_type, triangles() as triangle_type)
    for i% = shape.firstTriangleIndex to shape.lastTriangleIndex
        triangles(i%).a.x = triangles(i%).a.x * resizeFactor
        triangles(i%).a.y = triangles(i%).a.y * resizeFactor
        triangles(i%).b.x = triangles(i%).b.x * resizeFactor
        triangles(i%).b.y = triangles(i%).b.y * resizeFactor
        triangles(i%).c.x = triangles(i%).c.x * resizeFactor
        triangles(i%).c.y = triangles(i%).c.y * resizeFactor
        triangles(i%).center.x = (triangles(i%).a.x + triangles(i%).b.x + triangles(i%).c.x) / 3
        triangles(i%).center.y = (triangles(i%).a.y + triangles(i%).b.y + triangles(i%).c.y) / 3
    next i%
    computeShapeCenter shape, triangles()
end sub

'                                                 #####                                                  #####
'  ####   #####   ######    ##    #####  ######  #     #  #####   ######   ####   #  ######  #   ####   #     #  #    #    ##    #####   ######
' #    #  #    #  #        #  #     #    #       #        #    #  #       #    #  #  #       #  #    #  #        #    #   #  #   #    #  #
' #       #    #  #####   #    #    #    #####    #####   #    #  #####   #       #  #####   #  #        #####   ######  #    #  #    #  #####
' #       #####   #       ######    #    #             #  #####   #       #       #  #       #  #             #  #    #  ######  #####   #
' #    #  #   #   #       #    #    #    #       #     #  #       #       #    #  #  #       #  #    #  #     #  #    #  #    #  #       #
'  ####   #    #  ######  #    #    #    ######   #####   #       ######   ####   #  #       #   ####    #####   #    #  #    #  #       ######

sub createSpecificShape (shape as shape_type, shapes() as shape_type, triangles() as triangle_type)
    dim t as triangle_type
    dim as point_type p1, p2, p3, center
    read nbr%
    shapeIndex% = freeShape(shapes(), nbr%)
    if shapeIndex% > 0 then
        shape = shapes(shapeIndex%)
    else
        redim _preserve shapes(ubound(shapes) + 1) as shape_type
        shapeIndex% = ubound(shapes)
        redim _preserve triangles(0 to ubound(triangles) + nbr%) as triangle_type
        shape.lastTriangleIndex = ubound(triangles)
        shape.firstTriangleIndex = shape.lastTriangleIndex - nbr% + 1
    end if
    shape.id = shapeIndex%
    shape.life = nbr%
    do while nbr% > 0
        read p1.x, p1.y, p2.x, p2.y, p3.x, p3.y
        createTriangle t, p1, p2, p3
        t.shapeIndex = shapeIndex%
        t.id = shape.lastTriangleIndex - nbr% + 1
        triangles(shape.lastTriangleIndex - nbr% + 1) = t
        nbr% = nbr% - 1
    loop
end sub

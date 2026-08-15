'$INCLUDE:'../library/windowUtils.qlb'

'
' meteors mining
'
' first playable version (2025 november 12)
'

'''''''''''''''''''''''''''''''''''''''''''''''''
' compilation options
$let SHOW_INFOS = NO
$let SHOW_GLOBAL_MAP = YES
$let SHOW_RADIUS = NO
$let SHOW_DEBUG = NO
$let NO_MUSIC = YES
$console
'''''''''''''''''''''''''''''''''''''''''''''''''

const TAU = 8 * atn(1)

'$INCLUDE:'./context_type.bas'
'$INCLUDE:'./world_type.bas'
'$INCLUDE:'./point_type.bas'
'$INCLUDE:'./garbage_type.bas'
'$INCLUDE:'./segment_type.bas'
'$INCLUDE:'./triangle_type.bas'
'$INCLUDE:'./shape_type.bas'

type model_type
    firstTriangleIndex as integer
    lastTriangleIndex as integer
end type

const WHOIAM_METEOR = 0
const WHOIAM_BULLET = 1
const WHOIAM_PLAYER = 2
const WHOIAM_GEM = 3
const WHOIAM_STATION = 4
const WHOIAM_ALIEN = 5

type element_type
    id as integer
    name as string * 10
    color as _unsigned long
    count as integer
end type

const MOVING_MODE_INERTIAL = 0
const MOVING_MODE_4_WAYS = 1
const MOVING_MODE_DIRECTIONAL = 2

dim ctx as context_type

d& = _dest : _dest _console : ? "meteorsMining is starting" : _dest d&

ctx.VIEWPORT_WIDTH = 960 '576 '720 '320 '640 '800 '960 '1920 '1000
ctx.VIEWPORT_HEIGHT = 540 '324 '405 '240 '480 '600 '540 '1080 '800
ctx.EXTERNAL_BORDER = 1500
ctx.WORLD_WIDTH = ctx.VIEWPORT_WIDTH + 2 * ctx.EXTERNAL_BORDER
ctx.WORLD_HEIGHT = ctx.VIEWPORT_HEIGHT + 2 * ctx.EXTERNAL_BORDER
ctx.WORLD_MINX = (ctx.VIEWPORT_WIDTH - ctx.WORLD_WIDTH) \ 2
ctx.WORLD_MAXX = -ctx.WORLD_MINX + ctx.VIEWPORT_WIDTH
ctx.WORLD_MINY = (ctx.VIEWPORT_HEIGHT - ctx.WORLD_HEIGHT) \ 2
ctx.WORLD_MAXY = -ctx.WORLD_MINY + ctx.VIEWPORT_HEIGHT
ctx.DRAW_MINX = ctx.WORLD_MINX + ctx.EXTERNAL_BORDER - 100
ctx.DRAW_MAXX = ctx.WORLD_MAXX - ctx.EXTERNAL_BORDER + 100
ctx.DRAW_MINY = ctx.WORLD_MINY + ctx.EXTERNAL_BORDER - 100
ctx.DRAW_MAXY = ctx.WORLD_MAXY - ctx.EXTERNAL_BORDER + 100
ctx.STARS_COUNT = 100000
ctx.STARS_LAYERS = 15
ctx.SHAPES_COUNT = 150
ctx.TRIANGLES_IN_SHAPE_MIN = 6 '3
ctx.TRIANGLES_IN_SHAPE_MAX = 16 '8
ctx.TRIANGLE_BASE_MIN = 15 '10
ctx.TRIANGLE_BASE_MAX = 40 '30 'ctx.TRIANGLE_BASE_MIN
ctx.TRIANGLE_HEIGHT_MIN = 11 '7 'ctx.TRIANGLE_BASE_MIN * 0.86602540378444
ctx.TRIANGLE_HEIGHT_MAX = 22 '14 'ctx.TRIANGLE_HEIGHT_MIN
ctx.ATTEMPT_FPS = 600
ctx.FULL_SCREEN = 0 '-1

d& = _dest : _dest _console : ? "context declaration done" : _dest d&

dim as double ix, iy

ctx.movingMode = MOVING_MODE_DIRECTIONAL 'MOVING_MODE_INERTIAL

defineWorld ctx.world, ctx.WORLD_WIDTH, ctx.WORLD_HEIGHT, ctx.WORLD_MINY, ctx.WORLD_MINX, ctx.WORLD_MAXY, ctx.WORLD_MAXX

d& = _dest : _dest _console : ? "defineWorld done" : _dest d&

randomize timer

redim triangles(-1 to -1) as triangle_type
redim shapes(-1 to -1) as shape_type
redim garbages(-1 to -1) as garbage_type
redim elements(-1 to -1) as element_type

dim stars(1 to ctx.STARS_COUNT) as point_type

'''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''

dim shared sh1&, sh2&, sh3&, sh4&, sh5&, sh6&, sh7&, sh8&, sh9&
sh1& = _sndopen("../assets/mis4.ogg")
sh2& = _sndopen("../assets/exp7.ogg")
_sndvol sh2&, 0.5
'sh3& = _sndopen("../assets/Retro Blop 07.wav")
sh4& = _sndopen("../assets/exp7.ogg")
_sndvol sh4&, 0.05
sh5& = _sndopen("../assets/gem1.ogg")
_sndvol sh5&, 0.05
sh6& = _sndopen("../assets/bruitDeFond.ogg")
sh7& = _sndopen("../assets/music1.ogg")
sh8& = _sndopen("../assets/fffff.ogg")
sh9& = _sndcopy(sh8&)

dim shared shexp(0 to 9) as _unsigned long

shexp(0) = _sndopen("../assets/exp11.ogg")
shexp(1) = _sndopen("../assets/exp12.ogg")
shexp(2) = _sndopen("../assets/exp13.ogg")
shexp(3) = _sndopen("../assets/exp14.ogg")
shexp(4) = _sndopen("../assets/exp15.ogg")
shexp(5) = _sndopen("../assets/exp16.ogg")
shexp(6) = _sndopen("../assets/exp17.ogg")
shexp(7) = _sndopen("../assets/exp18.ogg")
shexp(8) = _sndopen("../assets/exp19.ogg")
shexp(9) = _sndopen("../assets/exp20.ogg")

dim shared txtr&, meteorTexture&, playerTexture&, stationTexture& 

txtr& = _loadimage("../assets/orange2.png",32)

playerTexture& = _loadimage("../assets/vaisseau.png",32)
'playerTexture& = _loadimage("../assets/whaoo.png",32)

'meteorTexture& = _loadimage("../assets/meteor16x16.jpeg",32)
meteorTexture& = _loadimage("../assets/meteor_surface_tile.png",32)
'meteorTexture& = _loadimage("../assets/meteor32x32.jpeg",32)

'stationTexture& = _loadimage("../assets/untitled.png",32)
'stationTexture& = _loadimage("../assets/metalRayures.png",32)
stationTexture& = _loadimage("../assets/station.png",32)
'stationTexture&  = _loadimage("../assets/meteor_reflets_bleu.png",32)

'''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''

_dest _console

prepare ctx, shapes(), triangles(), stars(), elements(), windowUtils()

screen _newimage(ctx.VIEWPORT_WIDTH, ctx.VIEWPORT_HEIGHT, 32)
_DisplayOrder _GLRender , _Software

if ctx.FULL_SCREEN then _fullscreen , _smooth

titleLoop ctx, stars(), ctx.world
mainLoop ctx, shapes(), triangles(), stars(), garbages(), elements(), windowUtils()

system

'$INCLUDE:'./mainLoop.bas'

'''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''

'                                                                 #####                                   ###   #
'  ####   ######  #    #  ######  #####     ##    #####  ######  #     #  #    #    ##    #####   ######  # #  #
' #    #  #       ##   #  #       #    #   #  #     #    #       #        #    #   #  #   #    #  #       ### #
' #       #####   # #  #  #####   #    #  #    #    #    #####    #####   ######  #    #  #    #  #####      #
' #  ###  #       #  # #  #       #####   ######    #    #             #  #    #  ######  #####   #         # ###
' #    #  #       #   ##  #       #   #   #    #    #    #       #     #  #    #  #    #  #       #        #  # #
'  ####   ######  #    #  ######  #    #  #    #    #    ######   #####   #    #  #    #  #       ######  #   ###

function generateShape% ( shapes() as shape_type, _
                          triangles() as triangle_type, _
                          nbr%, baseMin%, baseMax%, hauteurMin%, hauteurMax%, _
                          elements() as element_type)

    dim shape as shape_type
    dim t as triangle_type

    ' find a free shape (destroyed) with the same number of triangles then reuse it
    shapeIndex% = freeShape%(shapes(), nbr%)

    ' allocate a new shape if did not found a reusable
    if shapeIndex% < 0 then
        redim _preserve shapes(0 to ubound(shapes) + 1) as shape_type
        shapeIndex% = ubound(shapes)
    else
        reuse% = -1
        shape = shapes(shapeIndex%)
    end if

    ' first triangle
    generateTriangle t, baseMin%, baseMax%, hauteurMin%, hauteurMax%
    t.shapeIndex = shapeIndex%
    if reuse% then
        t.id = shape.firstTriangleIndex
        triangles(shape.firstTriangleIndex) = t
        shape.lastTriangleIndex = shape.firstTriangleIndex
    else
        redim _preserve triangles(0 to ubound(triangles) + 1) as triangle_type
        t.id = ubound(triangles)
        triangles(ubound(triangles)) = t
        shape.firstTriangleIndex = ubound(triangles)
        shape.lastTriangleIndex = ubound(triangles)
    end if
    shape.id = shapeIndex%
    shape.pointsUsageIndicator = "000"
    shape.shapeColor = _rgba32(rnd * 128 + 127, rnd * 128 + 127, rnd * 128 + 127, 64)

    dim pt1 as point_type, pt2 as point_type, pt3 as point_type, pt0 as point_type
    dim a as point_type, b as point_type, c as point_type
    dim bs%, p%, i%, cnt%

    ' Générer les autres triangles
    do while nbr% > 1
        bs% = chooseBorderSegment(shape, triangles())
        p% = bs% mod 3
        i% = bs% \ 3
        a = triangles(shape.firstTriangleIndex + i%).a
        b = triangles(shape.firstTriangleIndex + i%).b
        c = triangles(shape.firstTriangleIndex + i%).c
        if p% = 0 then
            pt1 = a: pt2 = b: pt3 = c
        elseif p% = 1 then
            pt1 = b: pt2 = c: pt3 = a
        else
            pt1 = c: pt2 = a: pt3 = b
        end if

        cnt% = 20
        do
            cnt% = cnt% - 1
            generateVertexOutsideTriangle pt1, pt2, pt3, hauteurMax% - rnd * (hauteurMax% - hauteurMin%), pt0
        loop while isVertexInnerShape(shape, triangles(), pt0) and cnt% > 0

        createTriangle t, pt1, pt2, pt0
        if isTriangleValid(shape, triangles(), t) then
            if rnd > 0.85 then
                t.element = int(rnd * (ubound(elements) + 1))
            end if
            t.shapeIndex = shapeIndex%
            if reuse% then
                shape.lastTriangleIndex = shape.lastTriangleIndex + 1
                t.id = shape.lastTriangleIndex
                triangles(shape.lastTriangleIndex) = t
            else
                redim _preserve triangles(0 to ubound(triangles) + 1) as triangle_type
                t.id = ubound(triangles)
                triangles(ubound(triangles)) = t
                shape.lastTriangleIndex = ubound(triangles)
            end if
            shape.pointsUsageIndicator = shape.pointsUsageIndicator + "100"
        end if
        nbr% = nbr% - 1
    loop

    shape.life = shape.lastTriangleIndex - shape.firstTriangleIndex + 1
 
    shapes(shapeIndex%) = shape

    $if SHOW_DEBUG = YES then
        h& = _dest
        _dest _console
        ? "generateShape - reuse:" resuse% " - shapeIndex:" shapeIndex%
        for t% = lbound(triangles) to ubound(triangles) : printTriangle triangles(t%), shapes(),0 : next t%
        _dest h&
    $end if
 
    ' return the index of the shape

    generateShape% = shapeIndex%

end function

'                                         #####
' #####  #####   #    #  #    #   ####   #  #  #
'   #    #    #  #    #  ##   #  #    #  #  #
'   #    #    #  #    #  # #  #  #        #####
'   #    #####   #    #  #  # #  #          #  #
'   #    #   #   #    #  #   ##  #    #  #  #  #
'   #    #    #   ####   #    #   ####    #####

function trunc$ (v!, p%)
    e% = int(v!)
    m% = 10 ^ p%
    d% = int(m% * (v! - int(v!)))
    trunc$ = _tostr$(e%) + "." + _tostr$(d%)
end function

'                          #####
' #####   ######   ####   #  #  #
' #    #  #       #    #  #  #
' #    #  #####   #        #####
' #    #  #       #  ###     #  #
' #    #  #       #    #  #  #  #
' #####   ######   ####    #####

function deg$ (a!)
    deg$ = _tostr$(int(360 * t.angle / TAU)) + "°"
end function

'                                    ######                           #                             ###   #
' #  #    #  #    #  ######  #####   #     #  #####     ##    #    #  #        #  #    #  #  #####  # #  #
' #  ##   #  ##   #  #       #    #  #     #  #    #   #  #   #    #  #        #  ##  ##  #    #    ### #
' #  # #  #  # #  #  #####   #    #  #     #  #    #  #    #  #    #  #        #  # ## #  #    #       #
' #  #  # #  #  # #  #       #####   #     #  #####   ######  # ## #  #        #  #    #  #    #      # ###
' #  #   ##  #   ##  #       #   #   #     #  #   #   #    #  ##  ##  #        #  #    #  #    #     #  # #
' #  #    #  #    #  ######  #    #  ######   #    #  #    #  #    #  #######  #  #    #  #    #    #   ###

function innerDrawLimit% (ctx as context_type, p as point_type)
    if p.x < ctx.DRAW_MINX _orelse _
       p.x > ctx.DRAW_MAXX _orelse _
       p.y < ctx.DRAW_MINY _orelse _
       p.y > ctx.DRAW_MAXY then
        innerDrawLimit% = 0
        exit function
    else
        innerDrawLimit% = -1
    end if
end function

'                                                   #######
' ######      # ######  ####  ##### #  ####  #    # #       #       ####  #    #
' #           # #      #    #   #   # #    # ##   # #       #      #    # #    #
' #####       # #####  #        #   # #    # # #  # #####   #      #    # #    #
' #           # #      #        #   # #    # #  # # #       #      #    # # ## #
' #      #    # #      #    #   #   # #    # #   ## #       #      #    # ##  ##
' ######  ####  ######  ####    #   #  ####  #    # #       ######  ####  #    #

sub ejectionFlow (shape as shape_type, angle as double, flowColor as _unsigned long, camera as point_type, world as world_type)
    dim p as point_type
    f% = rnd * 2 - 1
    r1% = shape.radius + 3 + rnd * 3
    r2% = r1% + 6
    r3% = r2% + 4
    c! = cos(shape.orientation + angle)
    s! = sin(shape.orientation + angle)
    x! = shape.position.x + shape.center.x + camera.x
    y! = shape.position.y + shape.center.y + camera.y
    p.x = r1% * c! + x!
    p.y = r1% * s! + y!
    normalizeWorldPosition p, world
    circle (p.x, p.y), 1 + f%, flowColor
    p.x = r2% * c! + x!
    p.y = r2% * s! + y!
    normalizeWorldPosition p, world
    circle (p.x, p.y), 2 + f%, flowColor
    p.x = r3% * c! + x!
    p.y = r3% * s! + y!
    normalizeWorldPosition p, world
    circle (p.x, p.y), 3 + f%, flowColor
end sub

'                                  #####
' #####   #####     ##    #    #  #     #  #    #    ##    #####   ######
' #    #  #    #   #  #   #    #  #        #    #   #  #   #    #  #
' #    #  #    #  #    #  #    #   #####   ######  #    #  #    #  #####
' #    #  #####   ######  # ## #        #  #    #  ######  #####   #
' #    #  #   #   #    #  ##  ##  #     #  #    #  #    #  #       #
' #####   #    #  #    #  #    #   #####   #    #  #    #  #       ######

sub drawShape ( ctx as context_type, _
                shape as shape_type, _
                triangles() as triangle_type, _
                camera as point_type, _
                elements() as element_type, _
                options as string )

    dim p as point_type

    if shape.life <= 0 then exit sub

    getBorderSegments shape, triangles()

    $if SHOW_INFOS = YES then
        p.x = shape.position.x + camera.x
        p.y = shape.position.y + camera.y
        normalizeWorldPosition p, ctx.world
        _printstring(p.x,p.y),_tostr$(shape.id)+"-"+_tostr$(shape.life)
    $end if

    $if SHOW_GLOBAL_MAP = YES then
        if shape.id = 0 then
            ''            line( ctx.VIEWPORT_WIDTH\2 - ctx.VIEWPORT_WIDTH\20 + ctx.WORLD_MINX\10, _
            ''                3*ctx.VIEWPORT_HEIGHT\4 - ctx.VIEWPORT_HEIGHT\20 + ctx.WORLD_MINY\10 ) - step ( ctx.WORLD_WIDTH\10, ctx.WORLD_HEIGHT\10 ),&H40A0A0A0,B
            ''            line( ctx.VIEWPORT_WIDTH\2 - ctx.VIEWPORT_WIDTH\20 , _
            ''                ctx.VIEWPORT_HEIGHT\2 - ctx.VIEWPORT_HEIGHT\20 ) - step ( ctx.VIEWPORT_WIDTH\10, ctx.VIEWPORT_HEIGHT\10 ),&H70A0A0A0,B,&B1100110011001100
        end if
        p.x = shape.position.x + camera.x
        p.y = shape.position.y + camera.y
        normalizeWorldPosition p, ctx.world
        c& = _iif(shape.id, &H80FF0000, &H8000FF00)
        circle (p.x \ 10 + ctx.VIEWPORT_WIDTH \ 2 - ctx.VIEWPORT_WIDTH \ 20, p.y \ 10 + ctx.VIEWPORT_HEIGHT \ 2 - ctx.VIEWPORT_HEIGHT \ 20), 1, shape.shapeColor
    $end if

    for i% = shape.firstTriangleIndex to shape.lastTriangleIndex
        p.x = triangles(i%).realCenter.x + camera.x
        p.y = triangles(i%).realCenter.y + camera.y
        normalizeWorldPosition p, ctx.world
        if shape.showAllParts _orelse _
           triangles(i%).life > 0 _andalso innerDrawLimit(ctx,p) then
            if shape.texture > 0 then
                drawTexturedTriangle triangles(i%), shape, camera, elements(), ctx.world
            else
                drawTriangle triangles(i%), shape.shapeColor, camera, elements(), ctx.world
            end if
        end if
    next i%

    $if SHOW_RADIUS = YES then
        ' display center and radius
        p.x = shape.center.x + shape.position.x + camera.x
        p.y = shape.center.y + shape.position.y + camera.y
        normalizeWorldPosition p, ctx.world
        circle (p.x,p.y),shape.radius,&H30FFFF00
        circle (p.x,p.y),2,&H80FFFF00
    $end if

    ' display shape position
    ''    p.x = shape.position.x + camera.x
    ''    p.y = shape.position.y + camera.y
    ''    normalizeWorldPosition p
    ''    line (p.x-1,p.y-1)-(p.x+1,p.y+1),&H80FF6600,BF

    ' display shape.life
    ''    _printstring (p.x,p.y), _tostr$(shape.life)

    if options = "D" then
        p.x = shape.position.x + camera.x
        p.y = shape.position.y + camera.y
        normalizeWorldPosition p, ctx.world
        _printstring (p.x,p.y), _tostr$(shape.id) + ":" + _
            _tostr$(shape.firstTriangleIndex) + "," + _
            _tostr$(shape.lastTriangleIndex)
        for i% = shape.firstTriangleIndex to shape.lastTriangleIndex
            _printstring (p.x,p.y+(i%-shape.firstTriangleIndex+1)*10),_tostr$(triangles(i%).life) + "/" + _
                _tostr$(triangles(i%).aShapeBorder)+"/"+_tostr$(triangles(i%).bShapeBorder)+ "/" + _ 
                _tostr$(triangles(i%).cShapeBorder)
        next i%
    end if

end sub

Sub RotoZoom3 (X As Long, Y As Long, Image As Long, xScale As Single, yScale As Single, radianRotation As Single)
    ' This assumes you have set your drawing location with _DEST or default to screen.
    ' X, Y - is where you want to put the middle of the image
    ' Image - is the handle assigned with _LOADIMAGE
    ' xScale, yScale - are shrinkage < 1 or magnification > 1 on the given axis, 1 just uses image size.
    ' These are multipliers so .5 will create image .5 size on given axis and 2 for twice image size.
    ' radianRotation is the Angle in Radian units to rotate the image
    ' note: Radian units for rotation because it matches angle units of other Basic Trig functions
    '       and saves a little time converting from degree.
    '       Use the _D2R() function if you prefer to work in degree units for angles.

    Dim px(3) As Single: Dim py(3) As Single ' simple arrays for x, y to hold the 4 corners of image
    Dim W&, H&, sinr!, cosr!, i&, x2&, y2& '   variables for image manipulation
    W& = _Width(Image&): H& = _Height(Image&)
    px(0) = -W& / 2: py(0) = -H& / 2 'left top corner
    px(1) = -W& / 2: py(1) = H& / 2 ' left bottom corner
    px(2) = W& / 2: py(2) = H& / 2 '  right bottom
    px(3) = W& / 2: py(3) = -H& / 2 ' right top
    sinr! = Sin(-radianRotation): cosr! = Cos(-radianRotation) ' rotation helpers
    For i& = 0 To 3 ' calc new point locations with rotation and zoom
        x2& = xScale * (px(i&) * cosr! + sinr! * py(i&)) + X: y2& = yScale * (py(i&) * cosr! - px(i&) * sinr!) + Y
        px(i&) = x2&: py(i&) = y2&
    Next
    _MapTriangle _seamless (0, 0)-(0, H& - 1)-(W& - 1, H& - 1), Image To(px(0), py(0))-(px(1), py(1))-(px(2), py(2)), 
    _MapTriangle _seamless (0, 0)-(W& - 1, 0)-(W& - 1, H& - 1), Image To(px(0), py(0))-(px(3), py(3))-(px(2), py(2)), 
End Sub

'                             #######                                                 #######                                             
' #####  #####    ##   #    #    #    ###### #    # ##### #    # #####  ###### #####     #    #####  #   ##   #    #  ####  #      ###### 
' #    # #    #  #  #  #    #    #    #       #  #    #   #    # #    # #      #    #    #    #    # #  #  #  ##   # #    # #      #      
' #    # #    # #    # #    #    #    #####    ##     #   #    # #    # #####  #    #    #    #    # # #    # # #  # #      #      #####  
' #    # #####  ###### # ## #    #    #        ##     #   #    # #####  #      #    #    #    #####  # ###### #  # # #  ### #      #      
' #    # #   #  #    # ##  ##    #    #       #  #    #   #    # #   #  #      #    #    #    #   #  # #    # #   ## #    # #      #      
' #####  #    # #    # #    #    #    ###### #    #   #    ####  #    # ###### #####     #    #    # # #    # #    #  ####  ###### ###### 

sub drawTexturedTriangle (t as triangle_type, shape as shape_type, camera as point_type, elements() as element_type, world as world_type)
    dim as point_type a, b, c, center
    a.x = t.realA.x + camera.x
    a.y = t.realA.y + camera.y
    normalizeWorldPosition a, world
    b.x = t.realB.x + camera.x
    b.y = t.realB.y + camera.y
    normalizeWorldPosition b, world
    c.x = t.realC.x + camera.x
    c.y = t.realC.y + camera.y
    normalizeWorldPosition c, world
    if t.life > 0 then
    tax = t.realA.x - t.realCenter.x
    tay = t.realA.y - t.realCenter.y
    tbx = t.realB.x - t.realCenter.x
    tby = t.realB.y - t.realCenter.y
    tcx = t.realC.x - t.realCenter.x
    tcy = t.realC.y - t.realCenter.y
    bs% = t.textureBasePosition
    larger = _iif(shape.width>shape.height,shape.width,shape.height)
    tw% = _width(shape.texture)
    ratio =  tw% / larger
    _MapTriangle _seamless (t.a.x*ratio - tw%/2, t.a.y*ratio - tw%/2)- _
                 (t.b.x*ratio - tw%/2, t.b.y*ratio - tw%/2)- _
                 (t.c.x*ratio - tw%/2, t.c.y*ratio - tw%/2), _
                 shape.texture to (a.x,a.y)-(b.x,b.y)-(c.x,c.y), ,_smooth
'    _MapTriangle (t.a.x*ratio - larger*2,t.a.y*ratio)-(t.b.x*ratio - larger*2,t.b.y*ratio)-(t.c.x*ratio - larger*2,t.c.y*ratio),shape.texture to (a.x,a.y)-(b.x,b.y)-(c.x,c.y), ,_smooth
'    _MapTriangle (511,511)-(0,511)-(255,0),shape.texture to (a.x,a.y)-(b.x,b.y)-(c.x,c.y), ,_smooth
    if t.element > 0 then
        center.x = t.realCenter.x + camera.x
        center.y = t.realCenter.y + camera.y
        normalizeWorldPosition center, world
        circle (center.x, center.y), 1.2, elements(t.element).color
    end if
    end if
end sub

'                                 #######
' #####   #####     ##    #    #     #     #####   #    ##    #    #   ####   #       ######
' #    #  #    #   #  #   #    #     #     #    #  #   #  #   ##   #  #    #  #       #
' #    #  #    #  #    #  #    #     #     #    #  #  #    #  # #  #  #       #       #####
' #    #  #####   ######  # ## #     #     #####   #  ######  #  # #  #  ###  #       #
' #    #  #   #   #    #  ##  ##     #     #   #   #  #    #  #   ##  #    #  #       #
' #####   #    #  #    #  #    #     #     #    #  #  #    #  #    #   ####   ######  ######

sub drawTriangle (t as triangle_type, shapeColor as _unsigned long, camera as point_type, elements() as element_type, world as world_type)
    dim as point_type a, b, c, center
    c& = _iif(t.collid, shapeColor and &H70FFFFFF, shapeColor)
    b& = _iif(t.collid, &H70000000, &HFF000000)
    a.x = t.realA.x + camera.x
    a.y = t.realA.y + camera.y
    normalizeWorldPosition a, world
    b.x = t.realB.x + camera.x
    b.y = t.realB.y + camera.y
    normalizeWorldPosition b, world
    c.x = t.realC.x + camera.x
    c.y = t.realC.y + camera.y
    normalizeWorldPosition c, world
    if t.life > 0 then
    tax = t.realA.x - t.realCenter.x
    tay = t.realA.y - t.realCenter.y
    tbx = t.realB.x - t.realCenter.x
    tby = t.realB.y - t.realCenter.y
    tcx = t.realC.x - t.realCenter.x
    tcy = t.realC.y - t.realCenter.y
'    db : ? tax "," tay "/" tbx "," tby "/" tcx "," tcy : de
    if t.textureBasePosition = 0 then t.textureBasePosition = int(rnd*(512-32))
    bs% = t.textureBasePosition
'    _MapTriangle (tax,tay)-(tbx,tby)-(tcx,tcy),txtr& to (a.x,a.y)-(b.x,b.y)-(c.x,c.y), ,_smoothshrunk
'    _MapTriangle (bs%-tax,255-tay)-(bs%-tbx,bs%-tby)-(bs%-tcx,bs%-tcy),txtr& to (a.x,a.y)-(b.x,b.y)-(c.x,c.y), ,_smooth
    _MapTriangle (511,511)-(0,511)-(255,0),txtr& to (a.x,a.y)-(b.x,b.y)-(c.x,c.y), ,_smooth
'        line (a.x, a.y)-(b.x, b.y), _iif(t.aShapeBorder, b& or c&, c&)
'        line -(c.x, c.y), _iif(t.bShapeBorder, b& or c&, c&)
'        line -(a.x, a.y), _iif(t.cShapeBorder, b& or c&, c&)
    else
        line (a.x, a.y)-(b.x, b.y), c&, , &B1010101010101010
        line -(c.x, c.y), c&, , &B1010101010101010
        line -(a.x, a.y), c&, , &B1010101010101010
    end if
    if t.element > 0 then
        center.x = t.realCenter.x + camera.x
        center.y = t.realCenter.y + camera.y
        normalizeWorldPosition center, world
        circle (center.x, center.y), 1.2, elements(t.element).color
    end if
    'd.x = t.realCenter.x + camera.x
    'd.y = t.realCenter.y + camera.y
    'c& = _rgba(rnd*255,rnd*255,rnd*255,128)
    'paint ( d.x, d.y ),c&, &HFFFFFFFF
end sub

'                                  #####                                   ######
' #####   #####     ##    #    #  #     #  #    #    ##    #####   ######  #     #  #  #####   ######   ####   #####  #  ####   #    #
' #    #  #    #   #  #   #    #  #        #    #   #  #   #    #  #       #     #  #  #    #  #       #    #    #    # #    #  ##   #
' #    #  #    #  #    #  #    #   #####   ######  #    #  #    #  #####   #     #  #  #    #  #####   #         #    # #    #  # #  #
' #    #  #####   ######  # ## #        #  #    #  ######  #####   #       #     #  #  #####   #       #         #    # #    #  #  # #
' #    #  #   #   #    #  ##  ##  #     #  #    #  #    #  #       #       #     #  #  #   #   #       #    #    #    # #    #  #   ##
' #####   #    #  #    #  #    #   #####   #    #  #    #  #       ######  ######   #  #    #  ######   ####     #    #  ####   #    #
                                                                                                                   
sub drawShapeDirection (shape as shape_type, triangles() as triangle_type, player%, camera as point_type, world as world_type)
    c& = _iif(player%, &HFF00FF00, shape.shapeColor)
    dim p as point_type
    p.x = shape.position.x + shape.center.x + camera.x
    p.y = shape.position.y + shape.center.y + camera.y
    normalizeWorldPosition p, world
    circle (p.x, p.y), 2, c&
    'circle (p.x,p.y), shape.radius, shape.shapeColor
    p.x = p.x + cos(shape.direction) * shape.radius
    p.y = p.y - sin(shape.direction) * shape.radius
    circle (p.x, p.y), 1, c&
end sub

'                                ######                                               #####                                    ##        ##
' #    #  ######  #    #  #####  #     #    ##    #  #    #  #####    ####   #    #  #     #   ####   #        ####   #####   #  #  #   #  #
' ##   #  #        #  #     #    #     #   #  #   #  ##   #  #    #  #    #  #    #  #        #    #  #       #    #  #    #      ##     ##
' # #  #  #####     ##      #    ######   #    #  #  # #  #  #####   #    #  #    #  #        #    #  #       #    #  #    #            ###
' #  # #  #         ##      #    #   #    ######  #  #  # #  #    #  #    #  # ## #  #        #    #  #       #    #  #####            #   # #
' #   ##  #        #  #     #    #    #   #    #  #  #   ##  #    #  #    #  ##  ##  #     #  #    #  #       #    #  #   #            #    #
' #    #  ######  #    #    #    #     #  #    #  #  #    #  #####    ####   #    #   #####    ####   ######   ####   #    #            ###  #

' Gets a rainbow color
' Computes an HSV color with then given hue parameter [0,360[ (saturation=1 and value=1)
' Then convert HSV to RGB then return the _RGB32 color
function nextRainbowColor~& (hue#)
    dim r as integer, g as integer, b as integer
    dim h_sector as double, f as double
    dim p as double, q as double, t as double
    dim i as integer
    hue# = hue# mod 360
    h_sector = hue# / 60#
    i = int(h_sector) ' secteur 0..5
    f = h_sector - i
    ' s = 1, v = 1  => p=0, q=1-f, t=f
    p = 0#
    q = 1# - f
    t = f
    select case i
        case 0
            r = 255: g = int(255 * t): b = 0
        case 1
            r = int(255 * q): g = 255: b = 0
        case 2
            r = 0: g = 255: b = int(255 * t)
        case 3
            r = 0: g = int(255 * q): b = 255
        case 4
            r = int(255 * t): g = 0: b = 255
        case else
            r = 255: g = 0: b = int(255 * q)
    end select
    nextRainbowColor~& = _rgb32(r, g, b)
end function

'                                                 #####                                   ###   #
'  ####   #####   ######    ##    #####  ######  #     #  #    #    ##    #####   ######  # #  #
' #    #  #    #  #        #  #     #    #       #        #    #   #  #   #    #  #       ### #
' #       #    #  #####   #    #    #    #####    #####   ######  #    #  #    #  #####      #
' #       #####   #       ######    #    #             #  #    #  ######  #####   #         # ###
' #    #  #   #   #       #    #    #    #       #     #  #    #  #    #  #       #        #  # #
'  ####   #    #  ######  #    #    #    ######   #####   #    #  #    #  #       ######  #   ###

function createShape%(ctx as context_type, shapes() as shape_type, _
            triangles() as triangle_type, _
            camera as point_type, _
            elements() as element_type )

    shapeIndex% = generateShape ( _
        shapes(), triangles(), _
        ctx.TRIANGLES_IN_SHAPE_MIN + (ctx.TRIANGLES_IN_SHAPE_MAX - ctx.TRIANGLES_IN_SHAPE_MIN)*rnd, _
        ctx.TRIANGLE_BASE_MIN, ctx.TRIANGLE_BASE_MAX,ctx.TRIANGLE_HEIGHT_MIN, ctx.TRIANGLE_HEIGHT_MAX, _
        elements() )

    dim p as point_type

    do
        x# = ctx.WORLD_WIDTH * rnd + ctx.WORLD_MINX
        y# = ctx.WORLD_HEIGHT * rnd + ctx.WORLD_MINY

        p.x = x# + camera.x
        p.y = y# + camera.y
        normalizeWorldPosition p, ctx.world

        if  ( p.x > 0 _andalso p.x < ctx.VIEWPORT_WIDTH ) _orelse _
            ( p.y > 0 _andalso p.y < ctx.VIEWPORT_HEIGHT ) then
        else
            exit do
        end if
    loop

    shapes(shapeIndex%).whoIam = WHOIAM_METEOR
    shapes(shapeIndex%).isCollider = -1
    shapes(shapeIndex%).position.x = x#
    shapes(shapeIndex%).position.y = y#
    shapes(shapeIndex%).direction = TAU * rnd
    shapes(shapeIndex%).velocity = 2 * rnd
    shapes(shapeIndex%).orientation = 0
    shapes(shapeIndex%).rotation = 0.01 - 0.02 * rnd
    shapes(shapeIndex%).texture = meteorTexture&
    computeShapeCenter shapes(shapeIndex%), triangles()

    createShape% = shapeIndex%

end function

'                                             #
'  ####  #####  ######   ##   ##### ######   # #   #      # ###### #    #
' #    # #    # #       #  #    #   #       #   #  #      # #      ##   #
' #      #    # #####  #    #   #   #####  #     # #      # #####  # #  #
' #      #####  #      ######   #   #      ####### #      # #      #  # #
' #    # #   #  #      #    #   #   #      #     # #      # #      #   ##
'  ####  #    # ###### #    #   #   ###### #     # ###### # ###### #    #

sub createAlien (shapes() as shape_type, triangles() as triangle_type)

    dim shape as shape_type

    alien:
    '$include:'../assets/gripper-meshes.bas'
    restore alien
    meshesToShape shape, shapes(), triangles()
    resizeShape 1 / 5, shape, triangles()
    shape.whoIam = WHOIAM_ALIEN
    shape.isCollider = -1
    shape.showAllParts = -1
    shape.shapeColor = &H80FFFF00
    shape.position.x = 100 'rnd * ctx.WORLD_WIDTH
    shape.position.y = 100 'rnd *  ctx.WORLD_HEIGHT
    shape.direction = 0
    shape.velocity = 1
    shape.orientation = 0
    shape.rotation = 0
    shape.center.x = 0
    shape.center.y = 0
    shape.targetCenter.x = 0
    shape.targetCenter.y = 0
    shape.radius = 10
    shapes(shape.id) = shape
    computeShapeCenter shapes(shape.id), triangles()
end sub

'                                                 #####
'  ####   #####   ######    ##    #####  ######  #     #  #####    ##    #####  #   ####   #    #
' #    #  #    #  #        #  #     #    #       #          #     #  #     #    #  #    #  ##   #
' #       #    #  #####   #    #    #    #####    #####     #    #    #    #    #  #    #  # #  #
' #       #####   #       ######    #    #             #    #    ######    #    #  #    #  #  # #
' #    #  #   #   #       #    #    #    #       #     #    #    #    #    #    #  #    #  #   ##
'  ####   #    #  ######  #    #    #    ######   #####     #    #    #    #    #   ####   #    #

sub createStation (shapes() as shape_type, triangles() as triangle_type)
    dim shape as shape_type
    dim t as triangle_type
    dim as point_type p1, p2, p3, center
    station:
    '$include:'../assets/station.bas'
'    data 2,14,14
'    data -2,-8,0,-12,2,-8,6,-11,6,-6,11,-6,8,-2,12,0,8,2,11,6,6,6,6,11,2,8,0,12
'    data 2,8,0,12,-2,8,-6,11,-6,6,-11,6,-8,2,-12,0,-8,-2,-11,-6,-6,-6,-6,-11,-2,-8,0,-12
    restore station
    meshesToShape shape, shapes(), triangles()
'    resizeShape 1/3, shape, triangles()
    resizeShape 1, shape, triangles()
    shape.whoIam = WHOIAM_STATION
    shape.isCollider = -1
    shape.shapeColor = &HFF808000
    shape.texture = stationTexture& 
    shape.position.x = 0
    shape.position.y = 0
    shape.direction = 0
    shape.velocity = 0
    shape.orientation = 0
    shape.rotation = 0 '0.01
    computeShapeCenter shapes(shapeId), triangles()
    shape.radius = 36
    shapes(shape.id) = shape
end sub

'                                                 #####
'  ####   #####   ######    ##    #####  ######  #     #  ######  #    #
' #    #  #    #  #        #  #     #    #       #        #       ##  ##
' #       #    #  #####   #    #    #    #####   #  ####  #####   # ## #
' #       #####   #       ######    #    #       #     #  #       #    #
' #    #  #   #   #       #    #    #    #       #     #  #       #    #
'  ####   #    #  ######  #    #    #    ######   #####   ######  #    #

sub createGem (shapes() as shape_type, triangles() as triangle_type, elements() as element_type, sourceTriangle as triangle_type)
    dim shape as shape_type
    dim t as triangle_type
    dim as point_type p1, p2, p3, center
    gem:
    data 1,4
    data 2,-2,-2,-2,2,2,-2,2
    restore gem
    meshesToShape shape, shapes(), triangles()
    for i% = shape.firstTriangleIndex to shape.lastTriangleIndex
        triangles(i%).element = sourceTriangle.element
    next i%
    shape.whoIam = WHOIAM_GEM
    shape.isCollider = -1
    shape.element = sourceTriangle.element
    if shape.element < lbound(elements) _orelse shape.element > ubound(elements) then
        h& = _dest: _dest _console: printTriangle sourceTriangle, shapes(), -1: _dest h&
    else
        shape.shapeColor = elements(shape.element).color
    end if
    sourceShapeIndex% = sourceTriangle.shapeIndex
    shape.life = shape.lastTriangleIndex - shape.firstTriangleIndex + 1
    center.x = sourceTriangle.center.x
    center.y = sourceTriangle.center.y
    rotation center, shapes(sourceShapeIndex%).center, shapes(sourceShapeIndex%).orientation
    shape.position.x = center.x + shapes(sourceShapeIndex%).position.x
    shape.position.y = center.y + shapes(sourceShapeIndex%).position.y
    shape.direction = shapes(sourceShapeIndex%).direction + rnd * TAU / 4 - TAU / 8
    shape.velocity = shapes(sourceShapeIndex%).velocity * rnd * 2 - 1
    shape.orientation = shapes(sourceShapeIndex%).orientation
    shape.rotation = shapes(sourceShapeIndex%).rotation
    shape.center.x = 0
    shape.center.y = 0
    shape.radius = 10
    shapes(shape.id) = shape
end sub

'                            ######
' ######  #  #####   ######  #     #  #    #  #       #       ######  #####
' #       #  #    #  #       #     #  #    #  #       #       #         #
' #####   #  #    #  #####   ######   #    #  #       #       #####     #
' #       #  #####   #       #     #  #    #  #       #       #         #
' #       #  #   #   #       #     #  #    #  #       #       #         #
' #       #  #    #  ######  ######    ####   ######  ######  ######    #

sub fireBullet(shapes() as shape_type, _
            triangles() as triangle_type)

    dim shape as shape_type
    dim t as triangle_type
    dim as point_type p1, p2, p3
    
    bullet:
    data 1,-2,-2,-2,2,2,0
    restore bullet
    createSpecificShape shape, shapes(), triangles()
    shape.whoIam = WHOIAM_BULLET
    shape.isCollider = -1
    shape.shapeColor = &HFFFF0000
    shape.position.x = shapes(0).position.x + shapes(0).targetCenter.x
    shape.position.y = shapes(0).position.y + shapes(0).targetCenter.y
    shape.direction = - shapes(0).orientation
    shape.velocity = shapes(0).velocity + 2
    shape.orientation = shapes(0).orientation
    shape.rotation = 0
    shape.center.x = 0
    shape.center.y = 0
    shape.radius = 10
    shapes(shape.id) = shape

end sub

' #####   #####   ######  #####     ##    #####   ######
' #    #  #    #  #       #    #   #  #   #    #  #
' #    #  #    #  #####   #    #  #    #  #    #  #####
' #####   #####   #       #####   ######  #####   #
' #       #   #   #       #       #    #  #   #   #
' #       #    #  ######  #       #    #  #    #  ######

sub prepare(ctx as context_type,  _
            shapes() as shape_type, _
            triangles() as triangle_type, _
            stars() as point_type, _
            elements() as element_type, _
            windowUtils() as windowUtils_type)

    dim shape as shape_type
    dim t as triangle_type
    dim as point_type p1, p2, p3

    elements:
    data 3,0,WATER,&HFF8080FF,1,IRON,&HFFC0C0C0,2,GOLD,&HFFD0D010
    restore elements
    read nbr%
    redim elements(1 to nbr%) as element_type
    do while nbr% > 0
        read elements(nbr%).id, elements(nbr%).name, elements(nbr%).color
        nbr% = nbr% - 1
    loop

    player:
'    data 4,-10,-10,-10,0,0,-5,0,-10,0,0,-5,0,5,0,-10,10,-10,0,0,5,0,0,5,0,-5,10,0,1
    data 1, -10,-10,-10,10,10,0,0
    restore player
    read nbr%
    shape.life = nbr%
    shape.firstTriangleIndex = 0
    do while nbr% > 0
        read p1.x, p1.y, p2.x, p2.y, p3.x, p3.y, el%
        createTriangle t, p1, p2, p3
        redim _preserve triangles(0 to ubound(triangles) + 1) as triangle_type
        t.id = ubound(triangles)
        t.element = el%
        triangles(ubound(triangles)) = t
        shape.lastTriangleIndex = ubound(triangles)
        nbr% = nbr% - 1
    loop
    shape.id = 0
    shape.whoIam = WHOIAM_PLAYER
    shape.showAllParts = -1
    shape.isCollider = -1
    shape.shapeColor = &H80FFFFFF
    shape.texture = playerTexture&
    shape.position.x = 0 'ctx.VIEWPORT_WIDTH / 2
    shape.position.y = 0 'ctx.VIEWPORT_HEIGHT / 2
    shape.direction = 0
    shape.velocity = 0
    shape.orientation = 0
    shape.rotation = 0
    shape.center.x = 0
    shape.center.y = 0
    shape.targetCenter.x = 0
    shape.targetCenter.y = 0
    shape.radius = 10
    ctx.player.cargoSize = 5
    ctx.player.cargoQty = 0
    redim _preserve shapes(0 to 0) as shape_type
    shapes(0) = shape
    computeShapeCenter shapes(0), triangles()

    for i& = 1 to ctx.STARS_COUNT
        plan% = (i& mod ctx.STARS_LAYERS) + 1
        stars(i&).x = rnd * ctx.WORLD_WIDTH * plan%
        stars(i&).y = rnd * ctx.WORLD_HEIGHT * plan%
    next i&

    createStation shapes(), triangles()

end sub

'                                  #
' #####  #  #####  #       ######  #         ####    ####   #####
'   #    #    #    #       #       #        #    #  #    #  #    #
'   #    #    #    #       #####   #        #    #  #    #  #    #
'   #    #    #    #       #       #        #    #  #    #  #####
'   #    #    #    #       #       #        #    #  #    #  #
'   #    #    #    ######  ######  #######   ####    ####   #

sub titleLoop (ctx as context_type, stars() as point_type, world as world_type)

    dim camera as point_type
    dim pt as point_type

    colorStep = 0
    const COLOR_STEPS = 48
    const COLOR_SPEED = 0.3
    fullscreen% = ctx.FULL_SCREEN

    $if NO_MUSIC <> YES then
        _sndloop sh7&
    $end if

    banner$ = _
        "     []    []                                                    []    [] []            []                       " + _
        "     [I]  [I]           []                   []                  [I]  [I]       []            []          []     " + _
        "     [][][][]  [III]  [IIII]  [III]   [III]   [III]   [III]      [][][][] []     [III]  []     [III]   [II]      " + _
        "     [] [] [] []   []   []   []   I] []   [] []   [] []          [] [] [] []     []  [] []     []  [] []  []     " + _
        "     []    [] [IIII]    []   [IIII]  []   [] []       [III]      []    [] []     []  [] []     []  []  [III]     " + _
        "     []    [] []        []   []      []   [] []           []     []    []  []    []  []  []    []  []     []     " + _
        "     []    []  [III]     [I]  [III]   [III]  []       [III]      []    []   [II] []  []   [II] []  [] []  []     " + _
        "                                                                                                       [II]      "
    bannerRows% = 8
    bannerColumns% = len(banner$) / bannerRows%
    dim bannerColors(0 to bannerColumns% - 1) as _unsigned long

    do
        _limit ctx.ATTEMPT_FPS

        ' keyboard control

        k$ = ucase$(inkey$)
        if k$ = "F" then
            fullscreen% = -1 - fullscreen%
            if fullscreen% then
                _fullscreen , _smooth
            else
                _fullscreen _off
            end if
            k$ = ""
        end if

        ' begins redraw

        _dest 0
        cls 1, &HFF000000, 0

        ' draws stars

        for i& = 1 to ctx.STARS_COUNT
            plan% = (i& mod ctx.STARS_LAYERS) + 1
            pt.x = (stars(i&).x + camera.x) * plan%
            pt.y = (stars(i&).y + camera.y) * plan%
            normalizePlanPosition pt, world, plan%
            'if innerDrawLimit(pt) then pset (pt.x, pt.y), _rgba32(255, 255, 255, rnd * 128 + 127)
            if innerDrawLimit(ctx,pt) then line (pt.x, pt.y)-(pt.x, pt.y), _rgba32(255, 255, 255, rnd * 128 + 127)
        next i&

        ' moves camera

        camera.y = camera.y + 1
        normalizeWorldPosition camera, world

        ' computes and rotates colors

        h# = colorStep * 360# / COLOR_STEPS
        colorStep = colorStep + COLOR_SPEED: if colorStep >= COLOR_STEPS then colorStep = colorStep - COLOR_STEPS
        bannerColors(bannerColumns% - 1) = nextRainbowColor(h#)
        for i% = 0 to bannerColumns% - 2: swap bannerColors(i%), bannerColors(i% + 1): next i%
        
        ' displays banner

        for j% = 0 to bannerRows% - 1
            for i% = 0 to bannerColumns% - 1
                color bannerColors(bannerColumns% - 1 - i%), &H00000000
                x% = (_width / bannerColumns%) * (i%)
                y% = 10 * (8 + j%)
                _printstring (x%, y%), mid$(banner$, 1 + j% * bannerColumns% + i%, 1)
            next i%
        next j%

        ' other infos

        m$ = "[ F for fullscreen ]"
        _printstring ((_width - _printwidth(m$)) / 2, y% + 20), m$
        m$ = "[ ESC to quit ]"
        _printstring ((_width - _printwidth(m$)) / 2, y% + 40), m$
        m$ = "[ Hit other key to launch ]"
        _printstring ((_width - _printwidth(m$)) / 2, y% + 60), m$

        _display
    loop until k$ <> ""

end sub

sub showBonus (ctx as context_type, message$, position as point_type)
    dim p as point_type
    p = position
    normalizeWorldPosition p, ctx.world
    _printstring (p.x, p.y), message$
end sub

sub sendMessage (ctx as context_type, message$)
    if ctx.messageQueue = "" and ctx.message = "" then ctx.messageDelay = -1 else ctx.messageDelay = 0.5
    ctx.messageQueue = ctx.messageQueue + message$ + "/"
    'drawMessage ctx
end sub

sub drawMessage (ctx as context_type)
    ctx.messageDelay = ctx.messageDelay - 1 / ctx.fps
    if ctx.messageDelay < 0 then
        ctx.messageDelay = 5
        p% = instr(ctx.messageQueue, "/")
        ctx.message2 = ctx.message1
        ctx.message1 = ctx.message
        ctx.message = mid$(ctx.messageQueue, 1, p% - 1)
        ctx.messageQueue = mid$(ctx.messageQueue, p% + 1)
    end if
    color &H7080FF80, &H00000000: locate ctx.VIEWPORT_HEIGHT \ _fontheight - 3, (ctx.VIEWPORT_WIDTH \ _fontwidth - len(ctx.message2)) \ 2: print ctx.message2;
    color &HA080FF80, &H00000000: locate ctx.VIEWPORT_HEIGHT \ _fontheight - 2, (ctx.VIEWPORT_WIDTH \ _fontwidth - len(ctx.message1)) \ 2: print ctx.message1;
    color &HFF80FF80, &H00000000: locate ctx.VIEWPORT_HEIGHT \ _fontheight - 1, (ctx.VIEWPORT_WIDTH \ _fontwidth - len(ctx.message)) \ 2: print ctx.message;
end sub

sub help (windowUtils() as windowUtils_type)
    wu% = windowUtils_openWindow(windowUtils(), (ctx.VIEWPORT_WIDTH - 320) \ 2, (ctx.VIEWPORT_HEIGHT - 240) \ 2, 320, 240, &HFF99DFFF, &HFFFFFFFF, &HD01AB2FF, &HFF1AB2FF)
    btn1% = windowUtils_createButton(windowUtils(wu%), "CLOSE", 20, 20)
    lbl1% = windowUtils_createLabel(windowUtils(wu%), "P : pause", 10, 10)
    lbl1% = windowUtils_createLabel(windowUtils(wu%), "S : show keys (this screen)", 10, 26)
    lbl1% = windowUtils_createLabel(windowUtils(wu%), "T : dump triangles to console", 10, 42)
    lbl1% = windowUtils_createLabel(windowUtils(wu%), "A : switch automatic shape generation", 10, 58)
    lbl1% = windowUtils_createLabel(windowUtils(wu%), "D : display data", 10, 74)
    lbl1% = windowUtils_createLabel(windowUtils(wu%), "F : switch fullscreen", 10, 90)
    lbl1% = windowUtils_createLabel(windowUtils(wu%), "G : generate a shape", 10, 106)
    lbl1% = windowUtils_createLabel(windowUtils(wu%), "K : kill all shapes", 10, 122)
    lbl2% = windowUtils_createLabel(windowUtils(wu%), "         ", 240, 5)
    do
        _limit 24
        windowUtils_trapMouseEvents windowUtils(wu%)
        windowUtils_updateLabel windowUtils(wu%), lbl2%, _tostr$(windowUtils(wu%).mousex) + "," + _tostr$(windowUtils(wu%).mousey)
        if windowUtils_buttonClicked(windowUtils(wu%), btn1%, _FALSE) = -1 then exit do
        windowUtils_refresh windowUtils(wu%)
    loop
    windowUtils_closeWindow windowUtils(wu%)
end sub

'''sub makeYourChoice (windowUtils() as windowUtils_type, ctx as context_type)
'''    wu% = windowUtils_openWindow(windowUtils(), (ctx.VIEWPORT_WIDTH - 320) \ 2, (ctx.VIEWPORT_HEIGHT - 240) \ 2, 320, 240, &HFF99DFFF, &HFFFFFFFF, &HD01AB2FF, &HFF1AB2FF)
'''    btn1% = windowUtils_createButton(windowUtils(wu%), "CLOSE", 20, 20)
'''    btn2% = windowUtils_createButton(windowUtils(wu%), "UNLOAD", 40, 20)
'''    lbl1% = windowUtils_createLabel(windowUtils(wu%), "*** DOCK STATION ***", 10, 10)
'''    lbl2% = windowUtils_createLabel(windowUtils(wu%), "         ", 260, 5)
'''    do
'''        _limit 24
'''        windowUtils_trapMouseEvents windowUtils(wu%)
'''        windowUtils_updateLabel windowUtils(wu%), lbl2%, _tostr$(windowUtils(wu%).mousex) + "," + _tostr$(windowUtils(wu%).mousey)
'''        if windowUtils_buttonClicked(windowUtils(wu%), btn1%, _FALSE) = -1 then exit do
'''        windowUtils_refresh windowUtils(wu%)
'''    loop
'''    windowUtils_closeWindow windowUtils(wu%)
'''end sub


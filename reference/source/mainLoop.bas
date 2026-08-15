
'                            #
' #    #    ##    #  #    #  #         ####    ####   #####
' ##  ##   #  #   #  ##   #  #        #    #  #    #  #    #
' # ## #  #    #  #  # #  #  #        #    #  #    #  #    #
' #    #  ######  #  #  # #  #        #    #  #    #  #####
' #    #  #    #  #  #   ##  #        #    #  #    #  #
' #    #  #    #  #  #    #  #######   ####    ####   #

sub mainLoop(   ctx as context_type, _
                shapes() as shape_type, _
                triangles() as triangle_type, _
                stars() as point_type, _
                garbages() as garbage_type, _
                elements() as element_type, _
                windowUtils() as windowUtils_type   )

    dim camera as point_type
    dim pt as point_type

    ctx.playerAtStation = -1

    pause% = 0
    showData% = 0
    showInfo% = 0
    autoGenerateShape% = -1
    fullscreen% = ctx.FULL_SCREEN
    showKeys% = 0
    maxMeteorShapes% = 15
    thrust% = 0

    _sndloop sh6&
    _sndvol sh7&, 0.1
    _sndloop sh8&
    _sndpause sh8&

    ctx.fps = ctx.ATTEMPT_FPS
    t# = timer(.001)

    do
        _limit ctx.ATTEMPT_FPS

        frames% = frames% + 1
        if timer(.001) >= t# + 1 then
            ctx.fps = frames%
            frames% = 0
            t# = timer(.001)
        end if

        keycode = inp(96)
        'if keycode <> inp(96) then keycode = 0
        k$ = ucase$(inkey$)
        '
        if k$ = chr$(0) + chr$(59) then showKeys% = -1 - showKeys%
        ' A : switch automatic shape generation
        if k$ = "A" then
            autoGenerateShape% = -1 - autoGenerateShape%
        end if
        ' C : create an alien
        if k$ = "C" then
            createAlien shapes(), triangles()
        end if
        ' D : display data
        if k$ = "D" then showData% = -1 - showData%
        ' F :switch fullscreen
        if k$ = "F" then
            fullscreen% = -1 - fullscreen%
            if fullscreen% then
                _fullscreen , _smooth
                sendMessage ctx, "FULLSCREEN"
            else
                _fullscreen _off
                sendMessage ctx, "WINDOWED"
            end if
        end if
        ' G : generate a shape
        if k$ = "G" then
            i% = createShape(ctx, shapes(), triangles(), camera, elements())
            shapes(i%).position.x = shapes(O).position.x + ctx.VIEWPORT_WIDTH \ 4
            shapes(i%).position.y = shapes(O).position.y
            shapes(i%).velocity = 0
            'shapes(i%).rotation = 0
            'shapes(i%).orientation = TAU/8
        end if
        ' I : display info
        if k$ = "I" then showInfo% = -1 - showInfo%
        ' K : kill all shapes
        if k$ = "K" then
            for i% = 1 to ubound(shapes)
                for j% = shapes(i%).firstTriangleIndex to shapes(i%).lastTriangleIndex
                    triangles(j%).life = 0
                next j%
                shapes(i%).life = 0
            next i%
            aliveShapes% = 0
            aliveTriangles% = 0
        end if
        ' M : mute music
        if k$ = "M" then
            if _sndpaused(sh7&) then _sndloop sh7& else _sndpause sh7&
        end if
        ' P : pause
        if k$ = "P" then pause% = -1 - pause%
        ' S : show keys
        if k$ = "S" then showKeys% = -1 - showKeys%
        ' T : dump triangles to console
        if k$ = "T" then
            h& = _dest
            _dest _console
            for t% = lbound(triangles) to ubound(triangles): printTriangle triangles(t%), shapes(), 0: next t%
            _dest h&
        end if
        k$ = ""

        ctx.player.thrust = 0
        if ctx.player.fire > 0 then ctx.player.fire = ctx.player.fire - 1

        select case ctx.movingMode
            case MOVING_MODE_DIRECTIONAL
                select case keycode
                    case 72 'up
                        shapes(0).velocity = shapes(0).velocity + 60 * 0.05 / ctx.fps
                        ctx.player.thrust = 0.1
                        ctx.player.thrusted = -5
                    case 77 'right
                        shapes(0).direction = shapes(0).direction - 60 * (TAU / 210) / ctx.fps
                        shapes(0).orientation = -shapes(0).direction
                    case 80 'down
                        shapes(0).velocity = _iif(shapes(0).velocity > 0, shapes(0).velocity - 60 * 0.05 / ctx.fps, 0)
                        if shapes(0).velocity > 0 then ctx.player.revertThrusted = -5
                    case 75 'left
                        shapes(0).direction = shapes(0).direction + 60 * (TAU / 210) / ctx.fps
                        shapes(0).orientation = -shapes(0).direction
                    case 42, 54 'left shift,  right shift
                        if ctx.player.fire = 0 then
                            ' fire a bullet
                            _sndplay sh1&
                            fireBullet shapes(), triangles()
                            ctx.player.fire = ctx.fps / 3
                            ctx.bulletsFired = ctx.bulletsFired + 1
                        end if
                end select
            case MOVING_MODE_INERTIAL
                select case keycode
                    case 72
                        ctx.player.thrust = 0.1
                        ctx.player.thrusted = -5
                        dx1# = cos(shapes(0).direction) * shapes(0).velocity
                        dy1# = sin(shapes(0).direction) * shapes(0).velocity
                        dx2# = cos(shapes(0).orientation) * ( 60 * 0.05 / ctx.fps)
                        dy2# = sin(shapes(0).orientation) * (- 60 * 0.05 / ctx.fps)
                        dx# = dx1# + dx2#
                        dy# = dy1# + dy2#
                        shapes(0).direction = _atan2(dy#,dx#)
                        shapes(0).velocity = _hypot(dx#,dy#)
                    case 77
                        shapes(0).orientation = shapes(0).orientation + 60 * (TAU / 210) / ctx.fps
                    case 80
                        dx1# = cos(shapes(0).direction) * shapes(0).velocity
                        dy1# = sin(shapes(0).direction) * shapes(0).velocity
                        dx2# = cos(shapes(0).orientation) * ( - 60 * 0.05 / ctx.fps)
                        dy2# = sin(shapes(0).orientation) * ( 60 * 0.05 / ctx.fps)
                        dx# = dx1# + dx2#
                        dy# = dy1# + dy2#
                        shapes(0).direction = _atan2(dy#,dx#)
                        shapes(0).velocity = _hypot(dx#,dy#)
                        if shapes(0).velocity > 0 then ctx.player.revertThrusted = -5
                    case 75
                        shapes(0).orientation = shapes(0).orientation - 60 * (TAU / 210) / ctx.fps
                    case 42, 54 'left shift,  right shift
                        if ctx.player.fire = 0 then
                            ' fire a bullet
                            _sndplay sh1&
                            fireBullet shapes(), triangles()
                            ctx.player.fire = ctx.fps / 3
                            ctx.bulletsFired = ctx.bulletsFired + 1
                        end if
                end select
            case MOVING_MODE_4_WAYS
                select case keycode
                    case 72
                        ctx.player.thrust = 0.1
                        ctx.player.thrusted = -5
                        dx1# = cos(shapes(0).direction) * shapes(0).velocity
                        dy1# = sin(shapes(0).direction) * shapes(0).velocity
                        dx2# = 0
                        dy2# = 60 * 0.05 / ctx.fps
                        dx# = dx1# + dx2#
                        dy# = dy1# + dy2#
                        shapes(0).direction = _atan2(dy#,dx#)
                        shapes(0).velocity = _hypot(dx#,dy#)
                        shapes(0).orientation = - shapes(0).direction
                    case 77
                        dx1# = cos(shapes(0).direction) * shapes(0).velocity
                        dy1# = sin(shapes(0).direction) * shapes(0).velocity
                        dx2# = 60 * 0.05 / ctx.fps
                        dy2# = 0
                        dx# = dx1# + dx2#
                        dy# = dy1# + dy2#
                        shapes(0).direction = _atan2(dy#,dx#)
                        shapes(0).velocity = _hypot(dx#,dy#)
                        shapes(0).orientation = - shapes(0).direction
                    case 80
                        dx1# = cos(shapes(0).direction) * shapes(0).velocity
                        dy1# = sin(shapes(0).direction) * shapes(0).velocity
                        dx2# = 0
                        dy2# = - 60 * 0.05 / ctx.fps
                        dx# = dx1# + dx2#
                        dy# = dy1# + dy2#
                        shapes(0).direction = _atan2(dy#,dx#)
                        shapes(0).velocity = _hypot(dx#,dy#)
                        shapes(0).orientation = - shapes(0).direction
                        if shapes(0).velocity > 0 then ctx.player.revertThrusted = -5
                    case 75
                        dx1# = cos(shapes(0).direction) * shapes(0).velocity
                        dy1# = sin(shapes(0).direction) * shapes(0).velocity
                        dx2# = - 60 * 0.05 / ctx.fps
                        dy2# = 0
                        dx# = dx1# + dx2#
                        dy# = dy1# + dy2#
                        shapes(0).direction = _atan2(dy#,dx#)
                        shapes(0).velocity = _hypot(dx#,dy#)
                        shapes(0).orientation = - shapes(0).direction
                    case 42, 54 'left shift,  right shift
                        if ctx.player.fire = 0 then
                            ' fire a bullet
                            _sndplay sh1&
                            fireBullet shapes(), triangles()
                            ctx.player.fire = ctx.fps / 3
                            ctx.bulletsFired = ctx.bulletsFired + 1
                        end if
                end select
        end select

        if ctx.player.thrusted then
            if _sndpaused(sh8&) then _sndloop sh8&
            ctx.player.thrusted = ctx.player.thrusted + 1
        else
            if not _sndpaused(sh8&) then _sndpause sh8&
        end if

        if ctx.player.revertThrusted then
            if _sndpaused(sh9&) then _sndloop sh9&
            ctx.player.revertThrusted = ctx.player.revertThrusted + 1
        else
            if not _sndpaused(sh9&) then _sndpause sh9&
        end if

        ' resets triangles collision indicator
        
        for i% = lbound(triangles) to ubound(triangles)
            triangles(i%).collid = 0
        next i%

        ' moves shapes

        if not pause% then
            for i% = lbound(shapes) to ubound(shapes)
                movingShape shapes(i%), triangles(), ctx.world, ctx.fps
            next i%
        end if

        ' moves garbages

        for i% = lbound(garbages) to ubound(garbages)
            movingGarbage garbages(i%), ctx.fps
        next i%

        ' detects collisions

        for i% = lbound(shapes) to ubound(shapes)
            ' no collision detection if the shape is not a collider
            if not shapes(i%).isCollider then _continue
            for j% = i% + 1 to ubound(shapes)
                ' no collision detection if the other shape is not a collider
                if not shapes(j%).isCollider then _continue
                ' no collision detection between player and it's own bullets
                if shapes(i%).whoIam = WHOIAM_PLAYER _andalso shapes(j%).whoIam = WHOIAM_BULLET then _continue
                ' collision detection only if shapes are not too distant
                xDist = abs(shapes(i%).position.x + shapes(i%).center.x - shapes(j%).position.x - shapes(j%).center.x)
                yDist = abs(shapes(i%).position.y + shapes(i%).center.y - shapes(j%).position.y - shapes(j%).center.y)
                sumRadius = shapes(i%).radius + shapes(j%).radius
                if xDist <= sumRadius _andalso yDist <= sumRadius then
                    ' do detection
                    if detectCollision(shapes(i%), shapes(j%), triangles()) then
                        ' no elastic collision beetween gem and ( player or meteor )
                        ' no elastic collision with station
                        if shapes(i%).whoIam = WHOIAM_GEM _andalso _
                           ( shapes(j%).whoIam = WHOIAM_PLAYER _orelse shapes(j%).whoIam = WHOIAM_METEOR ) _orelse _
                           shapes(j%).whoIam = WHOIAM_GEM _andalso _
                           ( shapes(i%).whoIam = WHOIAM_PLAYER _orelse shapes(i%).whoIam = WHOIAM_METEOR ) _orelse _
                           shapes(i%).whoIam = WHOIAM_STATION _orelse _
                           shapes(j%).whoIam = WHOIAM_STATION then
                        else
                            ' elastic collision => shapes react to the other one
                            resolveElasticCollision shapes(i%), shapes(j%)
                        end if
                    end if
                end if
            next j%
        next i%

        ' resolves collisions

        aliveTriangles% = 0
        previousShapeIndex% = -1
        for i% = lbound(triangles) to ubound(triangles)
            if triangles(i%).collid then
                if ( triangles(i%).collidBy = WHOIAM_PLAYER _andalso shapes(triangles(i%).shapeIndex).whoIam = WHOIAM_GEM _andalso _
                     ctx.player.cargoQty < ctx.player.cargoSize ) then
                    _sndplay sh5&
                    shapes(triangles(i%).shapeIndex).life = 0
                    triangles(i%).life = 0
                    elements(triangles(i%).element).count = elements(triangles(i%).element).count + 1
                    ctx.player.cargoQty = ctx.player.cargoQty + 1
                    if ctx.player.cargoQty >= ctx.player.cargoSize then
                        sendMessage ctx, "YOUR LOADING BAY IS FULL, YOU MUST UNLOAD IT AT THE STATION"
                    end if
                elseif (triangles(i%).collidBy = WHOIAM_GEM _andalso shapes(triangles(i%).shapeIndex).whoIam = WHOIAM_PLAYER) then
                elseif (triangles(i%).collidBy = WHOIAM_STATION _andalso shapes(triangles(i%).shapeIndex).whoIam = WHOIAM_PLAYER) then
                elseif (shapes(triangles(i%).shapeIndex).whoIam = WHOIAM_STATION) then
                else
                    triangles(i%).life = 0
                    shapes(triangles(i%).shapeIndex).life = shapes(triangles(i%).shapeIndex).life - 1
                    if shapes(triangles(i%).shapeIndex).whoIam = WHOIAM_PLAYER then
                        sendMessage ctx, "YOUR SPACESHIP IS DAMAGED, THE STATION CAN CARRY OUT REPAIRS"
                        sendMessage ctx, "REPAIRS ARE NOT FREE OF CHARGE"
                    end if
                    ' player and gem collision not resolve earlier because cargo bay is full
                    if triangles(i%).collidBy = WHOIAM_PLAYER _andalso shapes(triangles(i%).shapeIndex).whoIam = WHOIAM_GEM then
                        sendMessage ctx, "YOU CANNOT TAKE ANY ADDITIONAL RESOURCES, UNLOAD AT THE STATION"
                    end if
                    ' if player destroy a meteor then increase then number of meteors
                    if ( triangles(i%).collidBy = WHOIAM_BULLET _andalso shapes(triangles(i%).shapeIndex).whoIam = WHOIAM_METEOR _
                         _andalso shapes(triangles(i%).shapeIndex).life <= 0 ) then
                        ctx.meteorsDestroyed = ctx.meteorsDestroyed + 1
                        showBonus ctx, "R+1", shapes(triangles(i%).shapeIndex).position
                        if maxMeteorShapes% < ctx.SHAPES_COUNT then maxMeteorShapes% = maxMeteorShapes% + 1
                    end if
                    v! = (1 - _hypot( shapes(triangles(i%).shapeIndex).position.x - shapes(0).position.x, _
                                     shapes(triangles(i%).shapeIndex).position.y-shapes(0).position.y _
                                    ) / _hypot(ctx.WORLD_WIDTH,ctx.WORLD_HEIGHT)) ^3
                    s% = int(rnd * 10)
                    'h& = _dest : _dest _console : ? v! : _dest h&
                    sh& = shexp(s%)
                    _sndvol sh&, v!
                    _sndplay sh&
                    generateGarbages garbages(), triangles(i%), shapes()
                    if triangles(i%).element > 0 then
                        if triangles(i%).collidBy = WHOIAM_BULLET _andalso triangles(i%).element > 0 then
                            createGem shapes(), triangles(), elements(), triangles(i%)
                        end if
                    end if
                end if
                ''
                shapeIndex% = triangles(i%).shapeIndex
                if shapeIndex% <> previousShapeIndex% then
                    if shapes(triangles(i%).shapeIndex).whoIam = WHOIAM_STATION then
                    else
                        computeShapeCenter shapes(shapeIndex%), triangles()
                    end if
                    previousShapeIndex% = shapeIndex%
                end if
                ''
            end if
            if triangles(i%).life > 0 then aliveTriangles% = aliveTriangles% + 1
        next i%

        ' detect return to the base
        if abs(shapes(0).position.x - shapes(1).position.x) < 5 _andalso abs(shapes(0).position.y - shapes(1).position.y) < 5 then
            if ctx.playerAtStation = 0 then
                ctx.playerAtStation = -1
                ctx.playerEnterStation = -1
                shapes(0).velocity = 0
                sendMessage ctx, "YOU ARE DOCKED AT THE STATION"
                ' stop le bruit de moteur
                _sndpause sh8&
                _sndpause sh9&
 '                makeYourChoice windowUtils(), ctx
                r% = windowUtils_choiceBox(windowUtils(),"*** DOCK STATION ***","UNLOAD","CLOSE")
                t# = timer(.001)
            else
                for i% = lbound(elements) to ubound(elements)
                    elements(i%).count = 0
                next i%
                ctx.player.cargoQty = 0
                ctx.playerEnterStation = 0
                ctx.playerAtStation = -1
            end if
        else
            if ctx.playerAtStation = -1 then
                sendMessage ctx, "YOU ARE LIVING THE STATION"
            end if
            ctx.playerAtStation = 0
        end if

        ' computes camera (point of view) - player's shape is always centered

        camera.x = (ctx.VIEWPORT_WIDTH / 2) - (shapes(0).position.x + shapes(0).center.x)
        camera.y = (ctx.VIEWPORT_HEIGHT / 2) - (shapes(0).position.y + shapes(0).center.y)
        normalizeWorldPosition camera, ctx.world

        ' deletes bullets outer of draw area

        for i% = lbound(shapes) to ubound(shapes)
            if shapes(i%).life <= 0 then _continue
            if shapes(i%).whoIam = WHOIAM_BULLET then
                pt.x = shapes(i%).position.x + camera.x
                pt.y = shapes(i%).position.y + camera.y
                normalizeWorldPosition pt, ctx.world
                if ( pt.x < ctx.DRAW_MINX _orelse pt.x > ctx.DRAW_MAXX _orelse _
                     pt.y < ctx.DRAW_MINY _orelse pt.y > ctx.DRAW_MAXY            ) then
                    shapes(i%).life = 0
                    for j% = shapes(i%).firstTriangleIndex to shapes(i%).lastTriangleIndex
                        triangles(j%).life = 0
                        ctx.bulletsLost = ctx.bulletsLost + 1
                    next j%
                end if
            end if
        next i%

        ' adds a new shape

        if autoGenerateShape% _andalso aliveShapes% < maxMeteorShapes% _andalso rnd > 0.95 then
            i% = createShape(ctx, shapes(), triangles(), camera, elements())
        end if

        _dest 0
        cls , &HFF000000
        color &HFFFFFFFF, &H00000000

        ' draws stars

        for i& = 1 to ctx.STARS_COUNT
            plan% = (i& mod ctx.STARS_LAYERS) + 1
            pt.x = (stars(i&).x + camera.x) * plan%
            pt.y = (stars(i&).y + camera.y) * plan%
            normalizePlanPosition pt, ctx.world, plan%
            'if innerDrawLimit(pt) then pset (pt.x, pt.y), _rgba32(255, 255, 255, rnd * 128 + 127)
            if innerDrawLimit(ctx, pt) then line (pt.x, pt.y)-(pt.x, pt.y), _rgba32(255, 255, 255, rnd * 128 + 127)
        next i&

        ' draws shapes

        aliveShapes% = 0
        for i% = 1 to ubound(shapes)
            if shapes(i%).life <= 0 then _continue
            ' clean the undestroyed shapes because some are forget by the logical ôô
            t% = 0
            for j% = shapes(i%).firstTriangleIndex to shapes(i%).lastTriangleIndex
                if triangles(j%).life>0 then t%=t%+1
            next j%
            if t%=0 then shapes(i%).life = 0 : _continue
            '
            drawShape ctx, shapes(i%), triangles(), camera, elements(), _iif(showData% = -1, "D", "")
            aliveShapes% = aliveShapes% + 1
        next i%

        ' draws player

        drawShape ctx, shapes(0), triangles(), camera, elements(), _iif(showData% = -1, "D", "")

        ' draw thrust

        if ctx.player.thrusted then
            ejectionFlow shapes(ctx.player.shapeIndex), TAU / 2, &HFFFFA000, camera, ctx.world
        end if

        if ctx.player.revertThrusted then
            ejectionFlow shapes(ctx.player.shapeIndex), 0, &HFF00A0FF, camera, ctx.world
        end if

        ' draws garbages

        for i% = lbound(garbages) to ubound(garbages)
            drawGarbage ctx, garbages(i%), camera
        next i%

        ' draws elements level

        e1 = elements(1).count
        e2 = e1 + elements(2).count
        e3 = e2 + elements(3).count
        for i% = 1 to ctx.player.cargoSize
            c& = _iif(i% <= e1, elements(1).color, _iif(i% <= e2, elements(2).color, _iif(i% <= e3, elements(3).color, &HFF808080)))
            x% = 11 * i% + 5
            circle (x%, 50), 5, c&
            if c& <> &HFF808080 then paint (x%, 50), c&
        next i%

        ' displays reputation and precision

        locate 1, 1: print "FPS:"; ctx.fps
        locate 1, 15: print "REPUTATION:"; _tostr$(ctx.meteorsDestroyed)
        if ctx.bulletsFired > 0 then locate 1, 30: print "PRECISION:"; _tostr$(int(100 * (1 - ctx.bulletsLost / ctx.bulletsFired))); "%"

        ' displays text informations

        drawMessage ctx

        if showInfo% then
            locate 1, 10: print "keycode:"; keycode
            locate 2, 1: print "auto generate shape:"; _iif(autoGenerateShape%, "ON", "OFF")
            Locate 1, 30: Print using "shapes:#### - triangles:#### - garbages:####"; _
                            ubound(shapes); ubound(triangles); ubound(garbages)
            locate 2, 30: print using "alive shapes:#### - alive triangles:#### "; aliveShapes%; aliveTriangles%
            locate 3, 1: print using "### ### ###"; elements(1).count; elements(2).count; elements(3).count;
        end if

        if showKeys% then 
            help windowUtils()
            showKeys% = 0
            t# = timer(.001)
        end if

        _display
    loop until keycode = 1
end sub

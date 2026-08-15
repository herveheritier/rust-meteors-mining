$INCLUDEONCE

'$INCLUDE:'./point_type.bas'

type segment_type
    a as point_type
    b as point_type
end type

'                                          #####                                                          ###                                                                ###   #
'  ####   #    #  ######   ####   #    #  #     #  ######   ####   #    #  ######  #    #  #####   ####    #   #    #  #####  ######  #####    ####   ######   ####   #####  # #  #
' #    #  #    #  #       #    #  #   #   #        #       #    #  ##  ##  #       ##   #    #    #        #   ##   #    #    #       #    #  #       #       #    #    #    ### #
' #       ######  #####   #       ####     #####   #####   #       # ## #  #####   # #  #    #     ####    #   # #  #    #    #####   #    #   ####   #####   #         #       #
' #       #    #  #       #       #  #          #  #       #  ###  #    #  #       #  # #    #         #   #   #  # #    #    #       #####        #  #       #         #      # ###
' #    #  #    #  #       #    #  #   #   #     #  #       #    #  #    #  #       #   ##    #    #    #   #   #   ##    #    #       #   #   #    #  #       #    #    #     #  # #
'  ####   #    #  ######   ####   #    #   #####   ######   ####   #    #  ######  #    #    #     ####   ###  #    #    #    ######  #    #   ####   ######   ####     #    #   ###

function checkSegmentsIntersect% (s1 as segment_type, s2 as segment_type, ix as double, iy as double)
    const epsilon = 0.001
    
    ' tests bouding boxes
    dim as double x1min, x1max, y1min, y1max, x2min, x2max, y2min, y2max
    
    if s1.a.x < s1.b.x then x1min = s1.a.x: x1max = s1.b.x else x1min = s1.b.x: x1max = s1.a.x
    if s1.a.y < s1.b.y then y1min = s1.a.y: y1max = s1.b.y else y1min = s1.b.y: y1max = s1.a.y
    if s2.a.x < s2.b.x then x2min = s2.a.x: x2max = s2.b.x else x2min = s2.b.x: x2max = s2.a.x
    if s2.a.y < s2.b.y then y2min = s2.a.y: y2max = s2.b.y else y2min = s2.b.y: y2max = s2.a.y
    
    ' no intersection
    if x1max < x2min or x2max < x1min or y1max < y2min or y2max < y1min then
        checkSegmentsIntersect% = 0
        exit function
    end if
    
    ' vertex testing
    if (abs(s1.a.x - s2.a.x) < epsilon and abs(s1.a.y - s2.a.y) < epsilon) or _
       (abs(s1.a.x - s2.b.x) < epsilon and abs(s1.a.y - s2.b.y) < epsilon) or _
       (abs(s1.b.x - s2.a.x) < epsilon and abs(s1.b.y - s2.a.y) < epsilon) or _
       (abs(s1.b.x - s2.b.x) < epsilon and abs(s1.b.y - s2.b.y) < epsilon) then
        checkSegmentsIntersect% = -3
        exit function
    end if
    
    ' compute vectors
    dim as double dx1, dy1, dx2, dy2
    dx1 = s1.b.x - s1.a.x
    dy1 = s1.b.y - s1.a.y
    dx2 = s2.b.x - s2.a.x
    dy2 = s2.b.y - s2.a.y
    
    ' test orientation of the segments
    dim as double det
    det = dx1 * dy2 - dy1 * dx2
    
    if abs(det) < epsilon then
        ' parallel or collinear segments
        if abs((s2.a.x - s1.a.x) * dy1 - (s2.a.y - s1.a.y) * dx1) < epsilon then
            ' collinear - overlap test
            if (x1min <= x2max and x2min <= x1max) and (y1min <= y2max and y2min <= y1max) then
                checkSegmentsIntersect% = -1
            else
                checkSegmentsIntersect% = 0
            end if
        else
            checkSegmentsIntersect% = 0
        end if
        exit function
    end if
    
    ' compute intersection parameters
    dim as double t1
    t1 = ((s2.a.x - s1.a.x) * dy2 - (s2.a.y - s1.a.y) * dx2) / det
    dim as double t2
    t2 = ((s2.a.x - s1.a.x) * dy1 - (s2.a.y - s1.a.y) * dx1) / det
    
    ' check if intersection is in the segments
    if t1 >= 0 and t1 <= 1 and t2 >= 0 and t2 <= 1 then
        ix = s1.a.x + t1 * dx1
        iy = s1.a.y + t1 * dy1
        checkSegmentsIntersect% = -1
    else
        checkSegmentsIntersect% = 0
    end if
end function

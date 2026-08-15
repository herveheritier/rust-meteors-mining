$INCLUDEONCE

type world_type
    width as integer
    height as integer
    minx as integer
    maxx as integer
    miny as integer
    maxy as integer
end type

sub defineWorld(world as world_type, width%, height%, upper%, left%, bottom%, right% )
    world.width = width%
    world.height = height%
    world.minx = left%
    world.maxx = right%
    world.miny = upper%
    world.maxy = bottom%
end sub
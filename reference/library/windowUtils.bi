'''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''

' windowUtils.BI

type windowUtils_type
    left as integer
    top as integer
    width as integer
    height as integer
    foregroundColor as _unsigned long
    hoverColor as _unsigned long
    backgroundColor as _unsigned long
    borderColor as _unsigned long
    previousImage as _unsigned long
    backgroundImage as _unsigned long
    windowIsOpen as integer
    mousex as integer
    mousey as integer
    buttonsCounter as integer
    buttonsData as string
    buttonsString as string
    labelsCounter as integer
    labelsData as string
    labelsString as string
end type

redim windowUtils(0) as windowUtils_type

const WINDOWUTILS_PADDING_LEFT = 10
const WINDOWUTILS_PADDING_RIGHT = 10
const WINDOWUTILS_PADDING_TOP = 10

'''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''''

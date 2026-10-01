package utils

import "fmt"

// ANSI color codes
const (
	Reset     = "\033[0m"
	Bold      = "\033[1m"
	Dim       = "\033[2m"
	Italic    = "\033[3m"
	Underline = "\033[4m"

	// Regular colors
	Black   = "\033[30m"
	Red     = "\033[31m"
	Green   = "\033[32m"
	Yellow  = "\033[33m"
	Blue    = "\033[34m"
	Magenta = "\033[35m"
	Cyan    = "\033[36m"
	White   = "\033[37m"

	// Background colors
	BgBlack   = "\033[40m"
	BgRed     = "\033[41m"
	BgGreen   = "\033[42m"
	BgYellow  = "\033[43m"
	BgBlue    = "\033[44m"
	BgMagenta = "\033[45m"
	BgCyan    = "\033[46m"
	BgWhite   = "\033[47m"
)

// LogColors maps log types to colors and emojis
var LogColors = map[string]struct {
	Color string
	Emoji string
}{
	"info":     {Cyan, "ℹ️"},
	"success":  {Green, "✅"},
	"warning":  {Yellow, "⚠️"},
	"error":    {Red, "❌"},
	"debug":    {Magenta, "🔍"},
	"user":     {Blue, "👤"},
	"content":  {Green, "📝"},
	"vote":     {Yellow, "👍"},
	"evidence": {Cyan, "🔗"},
	"system":   {White, "🔧"},
	"metric":   {Magenta, "📊"},
	"cache":    {Yellow, "💾"},
	"db":       {Blue, "🗄️"},
	"api":      {Green, "🌐"},
}

// ColorizeLog returns a colorized log message with emoji
func ColorizeLog(logType, message string) string {
	style, exists := LogColors[logType]
	if !exists {
		style = LogColors["info"]
	}
	return style.Color + style.Emoji + " " + message + Reset
}

// ColorizeMetric returns a colorized metric message
func ColorizeMetric(name string, value interface{}) string {
	return LogColors["metric"].Color + LogColors["metric"].Emoji + " " +
		Bold + name + Reset + ": " + Cyan + fmt.Sprintf("%v", value) + Reset
}

package utils

import (
	"fmt"
	"os"
	"sync"
	"time"
)

// LogLevel represents the severity of a log message
type LogLevel string

const (
	DEBUG   LogLevel = "DEBUG"
	INFO    LogLevel = "INFO"
	WARNING LogLevel = "WARNING"
	ERROR   LogLevel = "ERROR"
)

// Logger handles all logging operations
type Logger struct {
	mu        sync.Mutex
	logFile   *os.File
	logLevel  LogLevel
	isVerbose bool
}

// NewLogger creates a new logger instance
func NewLogger(logPath string, level LogLevel, verbose bool) (*Logger, error) {
	file, err := os.OpenFile(logPath, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0644)
	if err != nil {
		return nil, fmt.Errorf("failed to open log file: %v", err)
	}

	return &Logger{
		logFile:   file,
		logLevel:  level,
		isVerbose: verbose,
	}, nil
}

// formatLog creates a formatted log message
func (l *Logger) formatLog(level LogLevel, category, message string, data ...interface{}) string {
	timestamp := time.Now().Format("2006-01-02 15:04:05")
	formattedMsg := fmt.Sprintf(message, data...)
	return fmt.Sprintf("[%s] [%s] [%s] %s", timestamp, level, category, formattedMsg)
}

// log writes a log message to both file and console
func (l *Logger) log(level LogLevel, category, message string, data ...interface{}) {
	l.mu.Lock()
	defer l.mu.Unlock()

	logMsg := l.formatLog(level, category, message, data...)

	// Write to file (always plain text)
	fmt.Fprintln(l.logFile, logMsg)

	// Write to console (colorized)
	if l.isVerbose {
		fmt.Println(ColorizeLog(category, logMsg))
	}
}

// User activity logging
func (l *Logger) UserAction(userID, action string, data ...interface{}) {
	l.log(INFO, "user", fmt.Sprintf("User %s %s", userID, action), data...)
}

// Content activity logging
func (l *Logger) ContentAction(contentID, action string, data ...interface{}) {
	l.log(INFO, "content", fmt.Sprintf("Content %s %s", contentID, action), data...)
}

// Vote activity logging
func (l *Logger) VoteAction(userID, contentID, voteType string, data ...interface{}) {
	l.log(INFO, "vote", fmt.Sprintf("User %s voted %s on content %s", userID, voteType, contentID), data...)
}

// Evidence activity logging
func (l *Logger) EvidenceAction(evidenceID, action string, data ...interface{}) {
	l.log(INFO, "evidence", fmt.Sprintf("Evidence %s %s", evidenceID, action), data...)
}

// System logging
func (l *Logger) System(component, message string, data ...interface{}) {
	l.log(INFO, "system", fmt.Sprintf("[%s] %s", component, message), data...)
}

// API logging
func (l *Logger) API(method, path, status string, duration time.Duration, data ...interface{}) {
	l.log(INFO, "api", fmt.Sprintf("%s %s [%s] %v", method, path, status, duration), data...)
}

// Database logging
func (l *Logger) Database(operation, details string, duration time.Duration, data ...interface{}) {
	l.log(DEBUG, "db", fmt.Sprintf("%s: %s (%v)", operation, details, duration), data...)
}

// Cache logging
func (l *Logger) Cache(operation, key string, hit bool, data ...interface{}) {
	status := "MISS"
	if hit {
		status = "HIT"
	}
	l.log(DEBUG, "cache", fmt.Sprintf("%s %s: %s", operation, key, status), data...)
}

// Error logging
func (l *Logger) Error(component string, err error, data ...interface{}) {
	l.log(ERROR, "error", fmt.Sprintf("[%s] %v", component, err), data...)
}

// Warning logging
func (l *Logger) Warning(component string, message string, data ...interface{}) {
	l.log(WARNING, "warning", fmt.Sprintf("[%s] %s", component, message), data...)
}

// Debug logging
func (l *Logger) Debug(component string, message string, data ...interface{}) {
	if l.logLevel == DEBUG {
		l.log(DEBUG, "debug", fmt.Sprintf("[%s] %s", component, message), data...)
	}
}

// Close closes the log file
func (l *Logger) Close() error {
	return l.logFile.Close()
}

package communication

import (
	"os"
)

func CreateDir(path string) error {
	_, err := os.Stat(path)
	if os.IsNotExist(err) {
		return os.MkdirAll(path, os.ModePerm)
	}

	return err
}

package communication

import (
	"os"
	"path/filepath"
)

func CreateDir(path string) error {
	_, err := os.Stat(filepath.Dir(path))
	if os.IsNotExist(err) {
		return os.MkdirAll(filepath.Dir(path), os.ModePerm)
	}

	return err
}

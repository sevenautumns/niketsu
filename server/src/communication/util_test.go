package communication

import (
	"os"
	"testing"

	"github.com/stretchr/testify/require"
)

func TestCreateDir(t *testing.T) {
	dir := "testdir/"
	err := CreateDir(dir)
	require.NoError(t, err)
	require.DirExists(t, dir)

	longDir := "testdir2/test1234/abc"
	err = CreateDir(longDir)
	require.NoError(t, err)
	require.DirExists(t, dir)

	err = CreateDir(longDir)
	require.NoError(t, err)

	t.Cleanup(func() {
		os.RemoveAll(dir)
		os.RemoveAll("testdir2/")
	})
}
